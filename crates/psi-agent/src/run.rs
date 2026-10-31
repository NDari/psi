//! One run: turns until the model answers without tool calls, a budget is
//! reached, or the run is aborted.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use psi_ai::{
    AiError, ChatRequest, Client, Delta, FinishReason, RawToolCall, Response, ToolSchema, fix_call,
};
use psi_session::{Block, Entry, EntryBody, Role, SessionFile, StopReason, Usage};
use psi_tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::events::AgentEvent;
use crate::messages::{build_messages, plan_drop};

/// Limits on one run (§5). Reaching one stops a headless run.
#[derive(Debug, Clone)]
pub struct Budgets {
    /// Model requests per run (H-13).
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub max_wall: Option<Duration>,
    /// Failed tool calls in a row.
    pub max_consecutive_errors: u32,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            max_turns: 40,
            max_tool_calls: 200,
            max_wall: None,
            max_consecutive_errors: 20,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// The model name sent in requests.
    pub model: String,
    pub max_output: u64,
    /// The context window in tokens.
    pub n_ctx: u64,
    pub enable_thinking: Option<bool>,
    pub budgets: Budgets,
    pub system_prompt: String,
    pub tools: Vec<Tool>,
    pub home: Option<PathBuf>,
}

/// Messages for a running agent (pi parity): steering goes in before the
/// next turn; follow-ups wait until the model would otherwise stop.
#[derive(Debug, Clone, Default)]
pub struct Queues {
    pub steering: Arc<Mutex<VecDeque<String>>>,
    pub follow_up: Arc<Mutex<VecDeque<String>>>,
}

impl Queues {
    fn take(q: &Mutex<VecDeque<String>>) -> Vec<String> {
        q.lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The model answered without calling tools.
    Done,
    /// A budget was reached (the model's run, cut short).
    Budget(String),
    Aborted,
    /// The harness or the server failed.
    Failed(String),
}

impl Stop {
    pub fn name(&self) -> &'static str {
        match self {
            Stop::Done => "done",
            Stop::Budget(_) => "budget",
            Stop::Aborted => "aborted",
            Stop::Failed(_) => "failed",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Stop::Budget(r) | Stop::Failed(r) => Some(r),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub stop: Stop,
    /// The last answer's text.
    pub text: String,
    pub turns: u32,
    pub tool_calls: u32,
}

pub struct Agent {
    client: Client,
    session: SessionFile,
    cfg: AgentConfig,
    ctx: ToolCtx,
    schemas: Vec<ToolSchema>,
    queues: Queues,
    events: Option<UnboundedSender<AgentEvent>>,
}

/// A call after the shape-fix pipeline: the tool and its arguments, or the
/// refusal sent back instead.
type Planned = Result<(Tool, serde_json::Map<String, Value>), String>;

/// The pause before the one retry of a request that may pass.
const RETRY_DELAY: Duration = Duration::from_secs(1);

impl Agent {
    pub fn new(client: Client, session: SessionFile, cfg: AgentConfig, ctx: ToolCtx) -> Self {
        let schemas = cfg.tools.iter().map(|t| t.schema()).collect();
        Self {
            client,
            session,
            cfg,
            ctx,
            schemas,
            queues: Queues::default(),
            events: None,
        }
    }

    pub fn with_events(mut self, tx: UnboundedSender<AgentEvent>) -> Self {
        self.events = Some(tx);
        self
    }

    pub fn queues(&self) -> Queues {
        self.queues.clone()
    }

    /// Cancelling it aborts the request and tools in flight (Esc in the TUI).
    pub fn cancel_token(&self) -> CancellationToken {
        self.ctx.cancel.clone()
    }

    pub fn session(&self) -> &SessionFile {
        &self.session
    }

    fn emit(&self, ev: AgentEvent) {
        if let Some(tx) = &self.events {
            let _ = tx.send(ev);
        }
    }

    fn record(&mut self, body: EntryBody) -> Result<Entry, String> {
        let path = self.session.path().display().to_string();
        let e = self
            .session
            .append(body)
            .map_err(|e| format!("cannot write the session file {path}: {e}"))?
            .clone();
        self.emit(AgentEvent::Entry { entry: e.clone() });
        Ok(e)
    }

    fn user_message(text: &str) -> EntryBody {
        EntryBody::Message {
            role: Role::User,
            content: vec![Block::Text { text: text.into() }],
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        }
    }

    /// Runs the agent on one user input.
    pub async fn run(&mut self, input: &str) -> RunOutcome {
        self.emit(AgentEvent::RunStart {
            session: self.session.path().display().to_string(),
        });
        let mut out = RunOutcome {
            stop: Stop::Done,
            text: String::new(),
            turns: 0,
            tool_calls: 0,
        };
        out.stop = match self.run_inner(input, &mut out).await {
            Ok(s) => s,
            Err(e) => Stop::Failed(e),
        };
        self.emit(AgentEvent::RunEnd {
            stop: out.stop.name().into(),
            reason: out.stop.reason().map(str::to_owned),
            text: out.text.clone(),
            turns: out.turns,
            tool_calls: out.tool_calls,
        });
        out
    }

    async fn run_inner(&mut self, input: &str, out: &mut RunOutcome) -> Result<Stop, String> {
        if self.session.entries().is_empty() {
            self.record(EntryBody::Custom {
                custom_type: "session_info".into(),
                data: json!({"agent": "psi", "version": env!("CARGO_PKG_VERSION"),
                             "cwd": self.ctx.cwd, "model": self.cfg.model}),
            })?;
        }
        self.record(Self::user_message(input))?;
        let start = Instant::now();
        let b = self.cfg.budgets.clone();
        let mut consecutive_errors = 0u32;
        let mut overflow_retries = 0u32;
        loop {
            if self.ctx.cancel.is_cancelled() {
                return Ok(Stop::Aborted);
            }
            if out.turns >= b.max_turns {
                return Ok(Stop::Budget(format!(
                    "reached the limit of {} model requests in one run",
                    b.max_turns
                )));
            }
            if out.tool_calls >= b.max_tool_calls {
                return Ok(Stop::Budget(format!(
                    "reached the limit of {} tool calls in one run",
                    b.max_tool_calls
                )));
            }
            if let Some(w) = b.max_wall
                && start.elapsed() >= w
            {
                return Ok(Stop::Budget(format!(
                    "reached the time limit of {} seconds",
                    w.as_secs()
                )));
            }
            out.turns += 1;
            let turn = out.turns;
            self.emit(AgentEvent::TurnStart { turn });

            let resp = match self.request().await {
                Ok(r) => r,
                Err(AiError::ContextOverflow {
                    n_prompt_tokens,
                    n_ctx,
                }) => {
                    out.turns -= 1;
                    overflow_retries += 1;
                    if overflow_retries > 3 {
                        return Ok(Stop::Failed("the conversation still does not fit the context window after dropping old turns".into()));
                    }
                    self.drop_old_turns(n_prompt_tokens, n_ctx)?;
                    continue;
                }
                Err(AiError::Aborted) => {
                    self.record(EntryBody::Message {
                        role: Role::Assistant,
                        content: vec![],
                        usage: None,
                        stop_reason: Some(StopReason::Aborted),
                        error: Some("aborted".into()),
                        model: Some(self.cfg.model.clone()),
                    })?;
                    return Ok(Stop::Aborted);
                }
                Err(e) => {
                    self.record(EntryBody::Message {
                        role: Role::Assistant,
                        content: vec![],
                        usage: None,
                        stop_reason: Some(StopReason::Error),
                        error: Some(e.to_string()),
                        model: Some(self.cfg.model.clone()),
                    })?;
                    return Ok(Stop::Failed(e.to_string()));
                }
            };
            overflow_retries = 0;
            out.text = resp.text.clone();
            self.record_assistant(&resp)?;

            if resp.tool_calls.is_empty() {
                self.emit(AgentEvent::TurnEnd { turn });
                let follow = Queues::take(&self.queues.follow_up);
                if follow.is_empty() {
                    return Ok(Stop::Done);
                }
                for f in follow {
                    self.record(Self::user_message(&f))?;
                }
                continue;
            }

            let results = self.run_calls(&resp).await?;
            out.tool_calls += results.len() as u32;
            for failed in results {
                consecutive_errors = if failed { consecutive_errors + 1 } else { 0 };
            }
            self.emit(AgentEvent::TurnEnd { turn });
            if self.ctx.cancel.is_cancelled() {
                return Ok(Stop::Aborted);
            }
            if consecutive_errors >= b.max_consecutive_errors {
                return Ok(Stop::Budget(format!(
                    "{consecutive_errors} tool calls in a row failed"
                )));
            }
            for s in Queues::take(&self.queues.steering) {
                self.record(Self::user_message(&s))?;
            }
        }
    }

    async fn request(&mut self) -> Result<Response, AiError> {
        let messages = build_messages(&self.cfg.system_prompt, &self.session.branch());
        let req = ChatRequest {
            model: self.cfg.model.clone(),
            messages,
            tools: self.schemas.iter().map(ToolSchema::to_openai).collect(),
            max_tokens: self.cfg.max_output,
            enable_thinking: self.cfg.enable_thinking,
        };
        let events = self.events.clone();
        let mut on_delta = move |d: Delta| {
            if let (Delta::Text(t), Some(tx)) = (d, &events) {
                let _ = tx.send(AgentEvent::TextDelta { text: t });
            }
        };
        let mut retried = false;
        loop {
            match self
                .client
                .chat(&req, &self.ctx.cancel, &mut on_delta)
                .await
            {
                // One retry for errors that may pass: a dropped stream, a
                // server that is still loading. Template errors never pass.
                Err(AiError::Stream(_) | AiError::Connect { .. }) if !retried => {}
                Err(AiError::Http { status, .. })
                    if !retried && (status == 503 || status == 502) => {}
                other => return other,
            }
            retried = true;
            tokio::select! {
                () = tokio::time::sleep(RETRY_DELAY) => {}
                () = self.ctx.cancel.cancelled() => return Err(AiError::Aborted),
            }
        }
    }

    fn record_assistant(&mut self, resp: &Response) -> Result<(), String> {
        let mut content = Vec::new();
        if !resp.reasoning.is_empty() {
            content.push(Block::Thinking {
                thinking: resp.reasoning.clone(),
            });
        }
        if !resp.text.is_empty() {
            content.push(Block::Text {
                text: resp.text.clone(),
            });
        }
        for c in &resp.tool_calls {
            let parsed: Option<Value> = serde_json::from_str(&c.arguments).ok();
            let compact = parsed.as_ref().and_then(|v| serde_json::to_string(v).ok());
            let raw_differs = compact.as_deref() != Some(c.arguments.as_str());
            content.push(Block::ToolCall {
                id: c.id.clone(),
                name: c.name.clone(),
                arguments: parsed.unwrap_or_else(|| Value::String(c.arguments.clone())),
                raw: raw_differs.then(|| c.arguments.clone()),
            });
        }
        let stop_reason = Some(match resp.finish {
            FinishReason::Stop => StopReason::Stop,
            FinishReason::ToolCalls => StopReason::ToolUse,
            FinishReason::Length => StopReason::Length,
            FinishReason::Unknown if resp.tool_calls.is_empty() => StopReason::Stop,
            FinishReason::Unknown => StopReason::ToolUse,
        });
        let usage = resp.usage.map(|u| Usage {
            input: u.input,
            output: u.output,
            cache_read: u.cache_read,
            cache_write: 0,
        });
        self.record(EntryBody::Message {
            role: Role::Assistant,
            content,
            usage,
            stop_reason,
            error: None,
            model: Some(self.cfg.model.clone()),
        })?;
        if resp.timings.is_some() || resp.first_token_ms.is_some() {
            self.record(EntryBody::Custom {
                custom_type: "timing".into(),
                data: json!({"first_token_ms": resp.first_token_ms, "timings": resp.timings}),
            })?;
        }
        Ok(())
    }

    /// Runs the calls of one response: read-only ones side by side, the
    /// rest one at a time, in order. Returns, per call, whether it failed.
    async fn run_calls(&mut self, resp: &Response) -> Result<Vec<bool>, String> {
        let truncated = resp.finish == FinishReason::Length;
        let n = resp.tool_calls.len();
        let mut planned: Vec<(RawToolCall, Planned)> = Vec::new();
        for (i, call) in resp.tool_calls.iter().enumerate() {
            let fixed = fix_call(
                call,
                truncated && i + 1 == n,
                &self.schemas,
                &self.ctx.cwd,
                self.cfg.home.as_deref(),
            );
            let fixed = match fixed {
                Ok(f) => {
                    if !f.notes.is_empty() {
                        self.record(EntryBody::Custom {
                            custom_type: "shape_fix".into(),
                            data: json!({"tool_call_id": call.id, "notes": f.notes}),
                        })?;
                    }
                    match Tool::from_name(&f.name).filter(|t| self.cfg.tools.contains(t)) {
                        Some(t) => Ok((t, f.args)),
                        None => Err(format!("There is no tool named '{}'.", f.name)),
                    }
                }
                Err(msg) => Err(msg),
            };
            planned.push((call.clone(), fixed));
        }

        let mut outputs: Vec<Option<ToolOutput>> = vec![None; planned.len()];
        let mut i = 0;
        while i < planned.len() {
            // A batch of read-only calls, or one other call.
            let mut j = i + 1;
            if matches!(&planned[i].1, Ok((t, _)) if t.read_only()) {
                while j < planned.len() && matches!(&planned[j].1, Ok((t, _)) if t.read_only()) {
                    j += 1;
                }
            }
            for (call, p) in &planned[i..j] {
                if let Ok((t, args)) = p {
                    self.emit(AgentEvent::ToolStart {
                        id: call.id.clone(),
                        name: t.name().into(),
                        args: Value::Object(args.clone()),
                    });
                }
            }
            let ctx = &self.ctx;
            let futs = planned[i..j].iter().map(|(_, p)| async move {
                match p {
                    Ok((t, args)) => t.run(args, ctx).await,
                    Err(msg) => ToolOutput::err(msg.clone()),
                }
            });
            for (k, o) in join_all(futs).await.into_iter().enumerate() {
                outputs[i + k] = Some(o);
            }
            i = j;
        }

        let mut failed = Vec::new();
        for ((call, p), o) in planned.iter().zip(outputs) {
            let o = o.unwrap_or_else(|| ToolOutput::err("not run"));
            let name = match p {
                Ok((t, _)) => t.name().to_owned(),
                Err(_) => call.name.clone(),
            };
            self.record(EntryBody::ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: name.clone(),
                content: vec![Block::Text {
                    text: o.text.clone(),
                }],
                is_error: o.is_error,
            })?;
            self.emit(AgentEvent::ToolEnd {
                id: call.id.clone(),
                name,
                is_error: o.is_error,
            });
            failed.push(o.is_error);
        }
        Ok(failed)
    }

    /// Context overflow (§4.2): drop the oldest turns so the request fits,
    /// and record it.
    fn drop_old_turns(&mut self, n_prompt: u64, n_ctx: u64) -> Result<(), String> {
        let keep = plan_drop(&self.session.branch(), n_prompt, n_ctx, self.cfg.max_output)
            .ok_or_else(|| format!("the request ({n_prompt} tokens) does not fit the context window ({n_ctx} tokens), even with only the last turn"))?;
        self.record(EntryBody::Compaction {
            summary: String::new(),
            first_kept_id: keep,
            tokens_before: Some(n_prompt),
        })?;
        self.record(EntryBody::GuardEvent {
            guard: "context_overflow".into(),
            message: format!("the request was {n_prompt} tokens for a {n_ctx}-token window; dropped the oldest turns"),
            tool_call_id: None,
        })?;
        Ok(())
    }
}
