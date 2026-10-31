//! One run: turns until the model answers without tool calls, a budget is
//! reached, or the run is aborted.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use psi_ai::{
    AiError, ChatRequest, Client, Delta, FinishReason, RawToolCall, Response, ToolSchema, fix_call,
};
use psi_safety::always_ask::Rules;
use psi_safety::blocked::{blocked_message, blocked_path, dir_to_add};
use psi_safety::checkpoint::Checkpoints;
use psi_safety::policy::{Decision, FileFacts, Mode, Policy};
use psi_session::{Block, Entry, EntryBody, Role, SessionFile, StopReason, Usage};
use psi_tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::approval::{
    Approval, ApprovalRequest, Approver, BlockedChoice, BlockedRequest, Headless,
};
use crate::events::AgentEvent;
use crate::guards::{GuardAction, Guards};
use crate::messages::{build_messages, plan_drop};

/// Limits on one run (§5). Reaching one stops a headless run.
#[derive(Debug, Clone)]
pub struct Budgets {
    /// Model requests per run (H-13).
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub max_wall: Option<Duration>,
    /// Failed tool calls in a row before a nudge; twice this pauses (§8.6).
    pub max_consecutive_errors: u32,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            max_turns: 40,
            max_tool_calls: 200,
            max_wall: None,
            max_consecutive_errors: 10,
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
    /// A guard paused the run and nobody chose to go on (§8.6).
    Guard(String),
    /// The harness or the server failed.
    Failed(String),
}

impl Stop {
    pub fn name(&self) -> &'static str {
        match self {
            Stop::Done => "done",
            Stop::Budget(_) => "budget",
            Stop::Aborted => "aborted",
            Stop::Guard(_) => "guard",
            Stop::Failed(_) => "failed",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Stop::Budget(r) | Stop::Guard(r) | Stop::Failed(r) => Some(r),
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

/// What asks and what can be undone (psi-plan.md §8).
pub struct Safety {
    pub policy: Policy,
    /// `None` when checkpoints are off (no git).
    pub checkpoints: Option<Arc<Checkpoints>>,
    pub approver: Arc<dyn Approver>,
    /// Keys granted for the session: `--allow` entries, and `s` answers.
    pub grants: HashSet<String>,
    /// Mode, isolation and checkpoint state, recorded in `session_info`.
    pub info: Value,
}

impl Safety {
    /// Auto mode with the shipped list, no checkpoints, nobody to ask.
    pub fn headless(root: &Path, home: Option<&Path>, tmpdir: &Path) -> Self {
        Self {
            policy: Policy::new(Mode::Auto, Rules::shipped(), root, home, tmpdir),
            checkpoints: None,
            approver: Arc::new(Headless),
            grants: HashSet::new(),
            info: Value::Null,
        }
    }
}

pub struct Agent {
    client: Client,
    session: SessionFile,
    cfg: AgentConfig,
    ctx: ToolCtx,
    schemas: Vec<ToolSchema>,
    queues: Queues,
    events: Option<UnboundedSender<AgentEvent>>,
    safety: Safety,
    /// The last checkpoint capture failed: file writes ask (§7.2).
    capture_failing: bool,
    /// A checkpoint in this turn found changed files.
    turn_changed: bool,
    /// The token that aborts the current run; a new one per run after an abort.
    abort: Arc<Mutex<CancellationToken>>,
}

/// A call after the shape-fix pipeline: the tool and its arguments, or the
/// refusal sent back instead.
type Planned = Result<(Tool, serde_json::Map<String, Value>), String>;

/// A call's identity for the guards: the tool and its arguments, without
/// the free-text intent.
fn signature(call: &RawToolCall, p: &Planned) -> String {
    match p {
        Ok((t, args)) => {
            let mut a = args.clone();
            a.remove("intent");
            format!("{} {}", t.name(), Value::Object(a))
        }
        Err(_) => format!("{} {}", call.name, call.arguments),
    }
}

/// The pause before the one retry of a request that may pass.
const RETRY_DELAY: Duration = Duration::from_secs(1);

impl Agent {
    pub fn new(client: Client, session: SessionFile, cfg: AgentConfig, ctx: ToolCtx) -> Self {
        let schemas = cfg.tools.iter().map(|t| t.schema()).collect();
        let safety = Safety::headless(&ctx.cwd, cfg.home.as_deref(), &ctx.tmpdir);
        Self {
            client,
            session,
            cfg,
            ctx,
            schemas,
            queues: Queues::default(),
            events: None,
            safety,
            capture_failing: false,
            turn_changed: false,
            abort: Arc::new(Mutex::new(CancellationToken::new())),
        }
        .with_abort_token()
    }

    fn with_abort_token(self) -> Self {
        if let Ok(mut a) = self.abort.lock() {
            *a = self.ctx.cancel.clone();
        }
        self
    }

    /// Cancelling the token inside aborts the run going on now (Esc). It
    /// stays valid across runs: each run after an abort gets a fresh token.
    pub fn abort_handle(&self) -> Arc<Mutex<CancellationToken>> {
        self.abort.clone()
    }

    pub fn safety_mut(&mut self) -> &mut Safety {
        &mut self.safety
    }

    /// Text the model sees with the next request, as a `custom_message`
    /// (the output of `!cmd`, extension messages).
    pub fn add_context(&mut self, custom_type: &str, text: &str) -> Result<(), String> {
        self.record(EntryBody::CustomMessage {
            custom_type: custom_type.into(),
            content: vec![Block::Text { text: text.into() }],
        })
        .map(|_| ())
    }

    /// Continues in another session file (/new, /resume), with its own
    /// checkpoints.
    pub fn set_session(&mut self, session: SessionFile, checkpoints: Option<Arc<Checkpoints>>) {
        self.session = session;
        self.safety.checkpoints = checkpoints;
        self.capture_failing = false;
        if let Ok(mut f) = self.ctx.files.lock() {
            *f = psi_tools::FileTracker::default();
        }
    }

    pub fn with_safety(mut self, safety: Safety) -> Self {
        self.safety = safety;
        self
    }

    pub fn safety(&self) -> &Safety {
        &self.safety
    }

    pub fn session_mut(&mut self) -> &mut SessionFile {
        &mut self.session
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
        if self.ctx.cancel.is_cancelled() {
            self.ctx.cancel = CancellationToken::new();
            if let Ok(mut a) = self.abort.lock() {
                *a = self.ctx.cancel.clone();
            }
        }
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
                             "cwd": self.ctx.cwd, "model": self.cfg.model,
                             "safety": self.safety.info}),
            })?;
        }
        // At submit: this also captures the user's own edits (§7.2).
        self.capture("submit", None).await?;
        self.record(Self::user_message(input))?;
        let start = Instant::now();
        let b = self.cfg.budgets.clone();
        let mut guards = Guards::new(b.max_consecutive_errors);
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
                    let cut_off = (resp.finish == FinishReason::Length)
                        .then(|| guards.on_truncated(self.cfg.max_output))
                        .flatten();
                    if let Some(GuardAction::Nudge { guard, text }) = cut_off {
                        self.record(EntryBody::GuardEvent {
                            guard: guard.into(),
                            message: text.clone(),
                            tool_call_id: None,
                        })?;
                        self.record(EntryBody::CustomMessage {
                            custom_type: "guard".into(),
                            content: vec![Block::Text { text }],
                        })?;
                        continue;
                    }
                    // Headless only: a person at the keyboard can just say
                    // "go on", and a false alarm would cost them a turn.
                    if !self.safety.approver.interactive()
                        && let Some(GuardAction::Nudge { guard, text }) =
                            guards.on_answer(&resp.text)
                    {
                        self.record(EntryBody::GuardEvent {
                            guard: guard.into(),
                            message: text.clone(),
                            tool_call_id: None,
                        })?;
                        self.record(EntryBody::CustomMessage {
                            custom_type: "guard".into(),
                            content: vec![Block::Text { text }],
                        })?;
                        continue;
                    }
                    return Ok(Stop::Done);
                }
                for f in follow {
                    self.record(Self::user_message(&f))?;
                }
                continue;
            }

            let (n, pause) = self.run_calls(&resp, &mut guards).await?;
            out.tool_calls += n;
            self.emit(AgentEvent::TurnEnd { turn });
            if self.ctx.cancel.is_cancelled() {
                return Ok(Stop::Aborted);
            }
            if let Some(reason) = pause {
                let go_on = tokio::select! {
                    g = self.safety.approver.pause(reason.clone()) => g,
                    () = self.ctx.cancel.cancelled() => return Ok(Stop::Aborted),
                };
                if !go_on {
                    return Ok(Stop::Guard(reason));
                }
                guards.reset();
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
    /// rest one at a time, in order, each through the policy. Returns how
    /// many ran, and why a guard paused, if one did.
    async fn run_calls(
        &mut self,
        resp: &Response,
        guards: &mut Guards,
    ) -> Result<(u32, Option<String>), String> {
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

        self.turn_changed = false;
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
            if let (call, Ok((t, args))) = &planned[i]
                && !t.read_only()
            {
                outputs[i] = Some(self.run_one(&call.id, *t, args, &resp.text).await?);
                i = j;
                continue;
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

        // Guards see every call, then the turn (§8.6).
        let mut outputs: Vec<ToolOutput> = outputs
            .into_iter()
            .map(|o| o.unwrap_or_else(|| ToolOutput::err("not run")))
            .collect();
        let mut pause = None;
        let mut fired = Vec::new();
        for ((call, p), o) in planned.iter().zip(outputs.iter_mut()) {
            let sig = signature(call, p);
            for action in guards.on_call(&sig, o.is_error) {
                fired.push((call.id.clone(), action.clone()));
                match action {
                    GuardAction::Nudge { text, .. } => {
                        o.text.push('\n');
                        o.text.push_str(&text);
                    }
                    GuardAction::Pause { reason, .. } => {
                        pause.get_or_insert(reason);
                    }
                }
            }
        }
        if let Some(action) = guards.on_turn_end(self.turn_changed)
            && let (Some((call, _)), Some(o)) = (planned.last(), outputs.last_mut())
        {
            if let GuardAction::Nudge { text, .. } = &action {
                o.text.push('\n');
                o.text.push_str(text);
            }
            fired.push((call.id.clone(), action));
        }
        for (id, action) in fired {
            let (guard, message) = match action {
                GuardAction::Nudge { guard, text } => (guard, text),
                GuardAction::Pause { guard, reason } => (guard, format!("paused: {reason}")),
            };
            self.record(EntryBody::GuardEvent {
                guard: guard.into(),
                message,
                tool_call_id: Some(id),
            })?;
        }

        for ((call, p), o) in planned.iter().zip(outputs) {
            let name = match p {
                Ok((t, _)) => t.name().to_owned(),
                Err(_) => call.name.clone(),
            };
            self.record(EntryBody::ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: name.clone(),
                content: vec![Block::Text { text: o.text }],
                is_error: o.is_error,
            })?;
            self.emit(AgentEvent::ToolEnd {
                id: call.id.clone(),
                name,
                is_error: o.is_error,
            });
        }
        Ok((planned.len() as u32, pause))
    }

    /// One call that may change something: the policy decides whether it
    /// runs, asks or is refused; a blocked bash write offers the user a
    /// rerun (D-23); a checkpoint follows.
    async fn run_one(
        &mut self,
        id: &str,
        tool: Tool,
        args: &serde_json::Map<String, Value>,
        preceding: &str,
    ) -> Result<ToolOutput, String> {
        let decision = match tool {
            Tool::Bash => self
                .safety
                .policy
                .bash(args.get("command").and_then(Value::as_str).unwrap_or("")),
            Tool::Write | Tool::Edit => {
                let path = tool.target(args, &self.ctx).unwrap_or_default();
                let ignored = match self.safety.checkpoints.clone() {
                    Some(cp) => {
                        let p = path.clone();
                        tokio::task::spawn_blocking(move || cp.is_ignored(&p))
                            .await
                            .unwrap_or(false)
                    }
                    None => false,
                };
                let facts = FileFacts {
                    ignored,
                    capture_failing: self.capture_failing,
                };
                let shown = args.get("path").and_then(Value::as_str).unwrap_or("");
                self.safety
                    .policy
                    .file_write(tool.name(), &path, shown, facts)
            }
            _ => Decision::Allow,
        };
        match decision {
            Decision::Allow => {}
            Decision::Refuse(msg) => {
                self.policy(id, "refused", &msg)?;
                return Ok(ToolOutput::err(msg));
            }
            Decision::Ask { reason, keys } => {
                if keys.iter().all(|k| self.safety.grants.contains(k)) {
                    self.policy(id, "granted", &reason)?;
                } else {
                    let req = ApprovalRequest {
                        tool_call_id: id.into(),
                        tool: tool.name().into(),
                        args: Value::Object(args.clone()),
                        intent: args
                            .get("intent")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        preceding_text: preceding.into(),
                        reason: reason.clone(),
                        keys: keys.clone(),
                    };
                    let answer = tokio::select! {
                        a = self.safety.approver.approve(req) => a,
                        () = self.ctx.cancel.cancelled() => Approval::Deny,
                    };
                    match answer {
                        Approval::Once => self.policy(id, "approved", &reason)?,
                        Approval::Session => {
                            self.safety.grants.extend(keys);
                            self.policy(id, "approved_session", &reason)?;
                        }
                        Approval::Deny | Approval::DenyWith(_) => {
                            self.policy(id, "denied", &reason)?;
                            return Ok(ToolOutput::err(self.denial(&reason, answer)));
                        }
                    }
                }
            }
        }

        let mut out = tool.run(args, &self.ctx).await;
        if tool == Tool::Bash
            && out.is_error
            && let Some(sb) = self.ctx.sandbox.clone()
            && let Some(path) = blocked_path(&out.text, &sb.writable())
        {
            let dir = dir_to_add(&path);
            let req = BlockedRequest {
                tool_call_id: id.into(),
                command: args
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                path: path.clone(),
                dir: dir.clone(),
            };
            let choice = tokio::select! {
                c = self.safety.approver.blocked(req) => c,
                () = self.ctx.cancel.cancelled() => BlockedChoice::No,
            };
            let shown = path.display().to_string();
            match choice {
                BlockedChoice::RerunOutside => {
                    self.policy(id, "rerun_outside", &shown)?;
                    let again = psi_tools::bash::run(args.clone(), &self.ctx, true).await;
                    out = ToolOutput {
                        text: format!(
                            "[the user approved running it outside the sandbox]\n{}",
                            again.text
                        ),
                        is_error: again.is_error,
                    };
                }
                BlockedChoice::AddDir => match sb.add_dir(&dir, self.cfg.home.as_deref()) {
                    Ok(()) => {
                        self.policy(id, "dir_added", &dir.display().to_string())?;
                        let again = tool.run(args, &self.ctx).await;
                        out = ToolOutput {
                            text: format!(
                                "[the user made {} writable for this session; it ran again]\n{}",
                                dir.display(),
                                again.text
                            ),
                            is_error: again.is_error,
                        };
                    }
                    Err(e) => {
                        self.policy(id, "blocked", &e)?;
                        out.text.push_str(&format!("\n{}", blocked_message(&path)));
                    }
                },
                BlockedChoice::No => {
                    self.policy(id, "blocked", &shown)?;
                    out.text.push_str(&format!("\n{}", blocked_message(&path)));
                }
            }
        }
        if self.capture("tool", Some(id)).await? {
            self.turn_changed = true;
        }
        Ok(out)
    }

    /// The model's message for a denied call.
    fn denial(&self, reason: &str, answer: Approval) -> String {
        if let Approval::DenyWith(m) = answer {
            return format!("The user denied this call: {m}");
        }
        if self.safety.approver.interactive() {
            format!(
                "The user denied this call ({reason}). Don't retry it in another form; ask the user if you need it."
            )
        } else {
            format!(
                "Not run: {reason}. This run can't ask the user, so it was denied. Don't retry it in another form; say in your answer what you needed."
            )
        }
    }

    fn policy(&mut self, id: &str, decision: &str, reason: &str) -> Result<(), String> {
        self.record(EntryBody::PolicyDecision {
            tool_call_id: id.into(),
            decision: decision.into(),
            reason: reason.into(),
            rewrite: None,
        })
        .map(|_| ())
    }

    /// Snapshots the workspace (§7.2). A submit is always recorded, so
    /// /undo can find it; a tool step only when files changed. Returns
    /// whether they did. A failed capture makes file writes ask.
    async fn capture(&mut self, trigger: &str, call: Option<&str>) -> Result<bool, String> {
        let Some(cp) = self.safety.checkpoints.clone() else {
            return Ok(false);
        };
        let msg = match call {
            Some(c) => format!("{trigger} {c}"),
            None => trigger.to_owned(),
        };
        let res = tokio::task::spawn_blocking(move || cp.capture(&msg))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()));
        match res {
            Ok(c) => {
                self.capture_failing = false;
                tracing::debug!(
                    ms = c.elapsed.as_millis() as u64,
                    changed = c.changed,
                    "checkpoint"
                );
                if c.changed || trigger == "submit" {
                    self.record(EntryBody::Checkpoint {
                        commit: c.commit,
                        trigger: Some(trigger.into()),
                        tool_call_id: call.map(str::to_owned),
                    })?;
                }
                Ok(c.changed)
            }
            Err(e) => {
                self.capture_failing = true;
                self.record(EntryBody::GuardEvent {
                    guard: "checkpoint_failed".into(),
                    message: format!(
                        "the checkpoint failed, so file writes ask until one works: {e}"
                    ),
                    tool_call_id: call.map(str::to_owned),
                })?;
                Ok(false)
            }
        }
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
