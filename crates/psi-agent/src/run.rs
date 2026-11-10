//! One run: turns until the model answers without tool calls, a budget is
//! reached, or the run is aborted.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use psi_ai::{
    AiError, ChatRequest, Client, Delta, FinishReason, RawToolCall, Response, ToolSchema, fix_call,
};
use psi_safety::blocked::blocked_message;
use psi_safety::checkpoint::Checkpoints;
use psi_safety::policy::{Decision, Policy};
use psi_safety::refuse::Rules;
use psi_session::{Block, Entry, EntryBody, Role, SessionFile, StopReason, Usage};
use psi_tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::approval::{Approver, Headless};
use crate::call::decide;
use crate::compact::CompactionSettings;
use crate::events::AgentEvent;
use crate::generating::Generating;
use crate::guards::{GuardAction, Guards};
use crate::messages::{build_messages, plan_drop};
use crate::post_edit::{self, PostEditCheck};
use crate::prompt;

mod fit;

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
    /// `/label` typed during a run: written before the next entry, on the
    /// entry that was latest when it was typed (see [`Agent::flush_labels`]).
    pub labels: Arc<Mutex<VecDeque<String>>>,
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

/// What is refused and what can be undone (design-going-forward.md, goal 1).
pub struct Safety {
    pub policy: Policy,
    /// `None` when checkpoints are off (no git).
    pub checkpoints: Option<Arc<Checkpoints>>,
    pub approver: Arc<dyn Approver>,
    /// Isolation and checkpoint state, recorded in `session_info`.
    pub info: Value,
}

impl Safety {
    /// The shipped refuse list, no checkpoints, nobody at the keyboard.
    pub fn headless(root: &Path, home: Option<&Path>, tmpdir: &Path) -> Self {
        Self {
            policy: Policy::new(Rules::shipped(), root, home, tmpdir),
            checkpoints: None,
            approver: Arc::new(Headless),
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
    /// A checkpoint in this turn found changed files.
    turn_changed: bool,
    /// The token that aborts the current run; a new one per run after an abort.
    abort: Arc<Mutex<CancellationToken>>,
    /// Run after a turn whose write or edit calls changed files (D-36).
    post_edit: Option<PostEditCheck>,
    /// What the server says it runs (model file, window, build), for the
    /// session record: requests only name the alias (`local`).
    server: Option<Value>,
    compaction: CompactionSettings,
    /// The last request sent, to check a summary request extends it (D-38).
    last_request: Option<ChatRequest>,
    /// The context size when summarizing last failed: no new try until it
    /// grows by a tenth of the window.
    compact_failed_at: Option<u64>,
    /// `Compacting` was sent and its `Compacted` not yet.
    compacting: bool,
}

/// A call after the shape-fix pipeline: the tool and its arguments, or the
/// refusal sent back instead.
type Planned = Result<(Tool, serde_json::Map<String, Value>), String>;

/// A call's identity for the guards: the tool and its arguments, without
/// the free-text intent.
fn signature(call: &RawToolCall, p: &Planned) -> String {
    match p {
        Ok((c, args)) => {
            let mut a = args.clone();
            a.remove("intent");
            format!("{} {}", c.name(), Value::Object(a))
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
            turn_changed: false,
            abort: Arc::new(Mutex::new(CancellationToken::new())),
            post_edit: None,
            server: None,
            compaction: CompactionSettings::default(),
            last_request: None,
            compact_failed_at: None,
            compacting: false,
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

    /// The tools offered to the model.
    fn tool_names(&self) -> Vec<String> {
        self.cfg.tools.iter().map(|t| t.name().to_owned()).collect()
    }

    /// Every schema the shape fixes may resolve to, offered or not.
    fn all_schemas(&self) -> Vec<ToolSchema> {
        self.schemas.clone()
    }

    pub fn safety_mut(&mut self) -> &mut Safety {
        &mut self.safety
    }

    /// Text the model sees with the next request, as a `custom_message`
    /// (the output of `!cmd`).
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
        if let Ok(mut f) = self.ctx.files.lock() {
            *f = psi_tools::FileTracker::default();
        }
    }

    /// The tail size and summary form compaction uses (the `compaction` setting).
    pub fn with_compaction(mut self, settings: CompactionSettings) -> Self {
        self.compaction = settings;
        self
    }

    /// The server's model file, window and build, recorded in
    /// `session_info`, and again as a `server` entry when a later run
    /// finds the server changed (a resumed session, a restarted server).
    pub fn with_server(mut self, info: Option<Value>) -> Self {
        self.server = info;
        self
    }

    /// The model server came up after the agent was made without it (the
    /// TUI starts with none running): its window, the result cap that
    /// follows from it, its record, and whether to ask for progress.
    pub fn connect(
        &mut self,
        n_ctx: u64,
        max_result_chars: usize,
        server: Option<Value>,
        progress: bool,
    ) {
        self.cfg.n_ctx = n_ctx;
        self.ctx.max_result_chars = max_result_chars;
        self.server = server;
        self.client = self.client.clone().with_progress(progress);
    }

    /// The server as the session last recorded it.
    fn recorded_server(&self) -> Option<&Value> {
        self.session
            .entries()
            .iter()
            .rev()
            .find_map(|e| match &e.body {
                EntryBody::Custom { custom_type, data } if custom_type == "server" => Some(data),
                EntryBody::Custom { custom_type, data } if custom_type == "session_info" => {
                    data.get("server")
                }
                _ => None,
            })
    }

    /// The project's check command, run after turns that edited files.
    pub fn with_post_edit_check(mut self, check: Option<PostEditCheck>) -> Self {
        self.post_edit = check;
        self
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
        self.flush_labels()?;
        self.write(body)
    }

    /// Writes the labels queued during a run, each on the latest entry so
    /// far. `record` calls it before every entry, so a label lands on the
    /// entry that was latest when the user typed it; the TUI calls it once
    /// a run ends for any still queued.
    pub fn flush_labels(&mut self) -> Result<(), String> {
        let Some(target) = self.session.leaf().map(str::to_owned) else {
            return Ok(()); // nothing to mark yet: they wait for the first entry
        };
        for label in Queues::take(&self.queues.labels) {
            self.write(EntryBody::Label {
                target_id: target.clone(),
                label: Some(label),
            })?;
        }
        Ok(())
    }

    fn write(&mut self, body: EntryBody) -> Result<Entry, String> {
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

    /// After an abort, a fresh token for what runs next.
    fn renew_cancel(&mut self) {
        if self.ctx.cancel.is_cancelled() {
            self.ctx.cancel = CancellationToken::new();
            if let Ok(mut a) = self.abort.lock() {
                *a = self.ctx.cancel.clone();
            }
        }
    }

    /// Runs the agent on one user input.
    pub async fn run(&mut self, input: &str) -> RunOutcome {
        self.renew_cancel();
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
            let mut data = json!({"agent": "psi", "version": env!("CARGO_PKG_VERSION"),
                                  "cwd": self.ctx.cwd, "model": self.cfg.model,
                                  "safety": self.safety.info});
            if let Some(s) = &self.server {
                data["server"] = s.clone();
            }
            self.record(EntryBody::Custom {
                custom_type: "session_info".into(),
                data,
            })?;
        } else if let Some(s) = self.server.clone()
            && self.recorded_server() != Some(&s)
        {
            self.record(EntryBody::Custom {
                custom_type: "server".into(),
                data: s,
            })?;
        }
        // At submit: this also captures the user's own edits (§7.2).
        self.capture("submit", None).await?;
        self.record(Self::user_message(input))?;
        let start = Instant::now();
        let b = self.cfg.budgets.clone();
        // A limit the user goes past in the TUI moves on by the same amount.
        let (mut turn_limit, mut call_limit) = (b.max_turns, b.max_tool_calls);
        let mut guards = Guards::new(b.max_consecutive_errors);
        // A run that can't ask went on from a no-progress pause already.
        let mut loop_noted = false;
        let mut overflow_retries = 0u32;
        loop {
            if self.ctx.cancel.is_cancelled() {
                return Ok(Stop::Aborted);
            }
            if out.turns >= turn_limit {
                let reason = format!("reached the limit of {turn_limit} model requests in one run");
                if let Some(stop) = self.budget_reached(reason).await? {
                    return Ok(stop);
                }
                turn_limit = turn_limit.saturating_add(b.max_turns);
            }
            if out.tool_calls >= call_limit {
                let reason = format!("reached the limit of {call_limit} tool calls in one run");
                if let Some(stop) = self.budget_reached(reason).await? {
                    return Ok(stop);
                }
                call_limit = call_limit.saturating_add(b.max_tool_calls);
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

            if self.fit_context().await?
                && let Some(GuardAction::Pause { guard, reason }) = guards.on_compaction()
            {
                self.record(EntryBody::GuardEvent {
                    guard: guard.into(),
                    message: format!("paused: {reason}"),
                    tool_call_id: None,
                })?;
                if let Some(stop) = self.pause_run(reason).await? {
                    return Ok(stop);
                }
                guards.reset();
            }
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
                    self.drop_old_turns(n_prompt_tokens, n_ctx).await?;
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
                // Steering typed during the answer goes in now, as after a
                // tool turn; before, it waited and slipped into
                // the next run after its first tool call.
                let steer = Queues::take(&self.queues.steering);
                if !steer.is_empty() {
                    for s in steer {
                        self.record(Self::user_message(&s))?;
                    }
                    continue;
                }
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

            let (n, pause) = self
                .run_calls(&resp, &mut guards, out.tool_calls, call_limit)
                .await?;
            out.tool_calls += n;
            self.emit(AgentEvent::TurnEnd { turn });
            if self.ctx.cancel.is_cancelled() {
                return Ok(Stop::Aborted);
            }
            if let Some(reason) = pause {
                // Going on from a no-progress pause as things were brings
                // the loop back; a note that names the repeated calls
                // mostly doesn't (experiments.md, 2026-11-09).
                let note = guards
                    .take_loop_note()
                    .filter(|(r, _)| *r == reason)
                    .map(|(_, note)| note);
                // A run that can't ask gets the note once in place of the
                // stop; the next such pause stops it.
                let unasked = note.is_some() && !self.safety.approver.interactive() && !loop_noted;
                if !unasked && let Some(stop) = self.pause_run(reason).await? {
                    return Ok(stop);
                }
                guards.reset();
                if let Some(text) = note {
                    loop_noted = true;
                    let message = if unasked {
                        "this run can't ask, so it goes on once, with a note naming the repeated calls"
                    } else {
                        "going on with a note naming the repeated calls"
                    };
                    self.record(EntryBody::GuardEvent {
                        guard: "no_progress".into(),
                        message: message.into(),
                        tool_call_id: None,
                    })?;
                    self.record(EntryBody::CustomMessage {
                        custom_type: "guard".into(),
                        content: vec![Block::Text { text }],
                    })?;
                }
            }
            for s in Queues::take(&self.queues.steering) {
                self.record(Self::user_message(&s))?;
            }
        }
    }

    /// A budget ran out: headless runs stop; in the TUI the run pauses and
    /// asks (§5), and going on gives it as much again.
    async fn budget_reached(&mut self, reason: String) -> Result<Option<Stop>, String> {
        if !self.safety.approver.interactive() {
            return Ok(Some(Stop::Budget(reason)));
        }
        Ok(match self.pause_run(reason.clone()).await? {
            None => None,
            Some(Stop::Aborted) => Some(Stop::Aborted),
            Some(_) => Some(Stop::Budget(reason)),
        })
    }

    /// Asks whether to go on after a guard's pause or a budget; the stop
    /// when not. Each pause is recorded with how long it waited for the
    /// user, so a review can tell waiting from working (the cog trial's
    /// worker paused six times, and only gaps in the timestamps showed it).
    async fn pause_run(&mut self, reason: String) -> Result<Option<Stop>, String> {
        let started = Instant::now();
        let stop = tokio::select! {
            g = self.safety.approver.pause(reason.clone()) => (!g).then(|| Stop::Guard(reason.clone())),
            () = self.ctx.cancel.cancelled() => Some(Stop::Aborted),
        };
        let answer = match &stop {
            None => "go on",
            Some(Stop::Aborted) => "aborted",
            Some(_) => "stop",
        };
        self.record(EntryBody::Custom {
            custom_type: "pause".into(),
            data: json!({"reason": reason, "answer": answer,
                         "waited_ms": started.elapsed().as_millis() as u64}),
        })?;
        Ok(stop)
    }

    fn system_prompt(&self) -> String {
        prompt::with_tools_line(
            &self.cfg.system_prompt,
            &prompt::tools_line(&self.tool_names()),
        )
    }

    /// The next request, from the session branch.
    fn build_request(&self) -> ChatRequest {
        let messages = build_messages(&self.system_prompt(), &self.session.branch());
        let offered = self.tool_names();
        let tools = self
            .all_schemas()
            .iter()
            .filter(|s| offered.iter().any(|n| n == s.name))
            .map(ToolSchema::to_openai)
            .collect();
        ChatRequest {
            model: self.cfg.model.clone(),
            messages,
            tools,
            max_tokens: self.cfg.max_output,
            enable_thinking: self.cfg.enable_thinking,
        }
    }

    async fn request(&mut self) -> Result<Response, AiError> {
        let req = self.build_request();
        self.last_request = Some(req.clone());
        let events = self.events.clone();
        // Thinking and tool-call arguments, as sizes; a retry counts again
        // from nothing.
        let generating = Arc::new(Mutex::new(Generating::default()));
        let counts = generating.clone();
        let mut on_delta = move |d: Delta| {
            if let (Some(tx), Ok(mut g)) = (&events, counts.lock())
                && let Some(ev) = g.on(&d)
            {
                let _ = tx.send(ev);
            }
            match d {
                Delta::Text(t) => {
                    if let Some(tx) = &events {
                        let _ = tx.send(AgentEvent::TextDelta { text: t });
                    }
                }
                Delta::PromptProgress {
                    total,
                    cache,
                    processed,
                } => {
                    if let Some(tx) = &events {
                        let _ = tx.send(AgentEvent::PromptProgress {
                            total,
                            cache,
                            processed,
                        });
                    }
                }
                _ => {}
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
                Err(e @ (AiError::Stream(_) | AiError::Connect { .. })) if !retried => {
                    self.emit(AgentEvent::Retrying {
                        reason: e.to_string(),
                    });
                }
                Err(e @ AiError::Http { status, .. })
                    if !retried && (status == 503 || status == 502) =>
                {
                    self.emit(AgentEvent::Retrying {
                        reason: e.to_string(),
                    });
                }
                other => return other,
            }
            retried = true;
            if let Ok(mut g) = generating.lock() {
                *g = Generating::default();
            }
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
    /// Runs one response's calls; those past `allowed` (the rest of the
    /// run's tool-call budget) are refused, not run.
    async fn run_calls(
        &mut self,
        resp: &Response,
        guards: &mut Guards,
        used: u32,
        call_limit: u32,
    ) -> Result<(u32, Option<String>), String> {
        let allowed = call_limit.saturating_sub(used) as usize;
        let truncated = resp.finish == FinishReason::Length;
        let n = resp.tool_calls.len();
        let mut planned: Vec<(RawToolCall, Planned)> = Vec::new();
        for (i, call) in resp.tool_calls.iter().enumerate() {
            let schemas = self.all_schemas();
            let offered = self.tool_names();
            let fixed = fix_call(
                call,
                truncated && i + 1 == n,
                &schemas,
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
                    match Tool::from_name(&f.name)
                        .filter(|t| self.cfg.tools.contains(t))
                        .filter(|c| offered.iter().any(|n| n == c.name()))
                    {
                        Some(c) => Ok((c, f.args)),
                        None => Err(format!("There is no tool named '{}'.", f.name)),
                    }
                }
                Err(msg) => Err(msg),
            };
            // Past the budget: not run, though the response asked for it.
            let fixed = if i >= allowed {
                Err(format!(
                    "Not run: this run reached its limit of {call_limit} tool calls. Say in your answer what is left to do."
                ))
            } else {
                fixed
            };
            planned.push((call.clone(), fixed));
        }

        self.turn_changed = false;
        // Read-only calls the policy lets through run side by side (§8.1).
        let mut side_by_side = Vec::with_capacity(planned.len());
        for (_, p) in &planned {
            side_by_side.push(match p {
                Ok((c, args)) if c.read_only() => {
                    decide(&self.safety.policy, c, args, &self.ctx) == Decision::Allow
                }
                _ => false,
            });
        }
        let mut outputs: Vec<Option<ToolOutput>> = vec![None; planned.len()];
        let mut i = 0;
        while i < planned.len() {
            // A batch of read-only calls, or one other call.
            let mut j = i + 1;
            if side_by_side[i] {
                while j < planned.len() && side_by_side[j] {
                    j += 1;
                }
            }
            for (call, p) in &planned[i..j] {
                if let Ok((c, args)) = p {
                    self.emit(AgentEvent::ToolStart {
                        id: call.id.clone(),
                        name: c.name().into(),
                        args: Value::Object(args.clone()),
                    });
                }
            }
            if let (call, Ok((c, args))) = &planned[i]
                && !side_by_side[i]
            {
                outputs[i] = Some(self.run_one(&call.id, c, args).await?);
                i = j;
                continue;
            }
            let ctx = &self.ctx;
            let futs = planned[i..j].iter().map(|(_, p)| async move {
                match p {
                    Ok((c, args)) => c.run(args, ctx).await,
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
        let held = self.limit_step(&planned, &mut outputs)?;
        let mut pause = None;
        let mut fired = Vec::new();
        for (i, ((call, p), o)) in planned.iter().zip(outputs.iter_mut()).enumerate() {
            // Held back, not run: the guards don't count it.
            if held.contains(&i) {
                continue;
            }
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
        // A file changed: the checkpoint saw it, or a write or edit worked
        // (with checkpoints off there is no capture to see it).
        let wrote = planned
            .iter()
            .zip(&outputs)
            .any(|((_, p), o)| matches!(p, Ok((Tool::Write | Tool::Edit, _))) && !o.is_error);
        if let Some(action) = guards.on_turn_end(self.turn_changed || wrote)
            && let (Some((call, _)), Some(o)) = (planned.last(), outputs.last_mut())
        {
            match &action {
                GuardAction::Nudge { text, .. } => {
                    o.text.push('\n');
                    o.text.push_str(text);
                }
                GuardAction::Pause { reason, .. } => {
                    pause.get_or_insert(reason.clone());
                }
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
        self.post_edit_check(&planned, &mut outputs).await?;
        for ((call, p), o) in planned.iter().zip(outputs) {
            let name = match p {
                Ok((c, _)) => c.name().to_owned(),
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

    /// D-36: once per turn, after the last write or edit that worked, run the
    /// project's check and add its summary to that call's result.
    async fn post_edit_check(
        &mut self,
        planned: &[(RawToolCall, Planned)],
        outputs: &mut [ToolOutput],
    ) -> Result<(), String> {
        let Some(check) = self.post_edit.clone() else {
            return Ok(());
        };
        let last = planned
            .iter()
            .zip(outputs.iter())
            .rposition(|((_, p), o)| !o.is_error && matches!(p, Ok((Tool::Write | Tool::Edit, _))));
        let Some(i) = last else { return Ok(()) };
        if self.ctx.cancel.is_cancelled() {
            return Ok(());
        }
        let args = json!({"command": check.command, "timeout": check.timeout_s});
        let Value::Object(args) = args else {
            return Ok(());
        };
        let started = Instant::now();
        let out = psi_tools::bash::run(args, &self.ctx).await;
        self.record(EntryBody::Custom {
            custom_type: "post_edit_check".into(),
            data: json!({"tool_call_id": planned[i].0.id, "command": check.command,
                         "passed": !out.is_error, "ms": started.elapsed().as_millis() as u64}),
        })?;
        let o = &mut outputs[i];
        o.text.push_str("\n\n");
        o.text.push_str(&post_edit::summary(&check.command, &out));
        Ok(())
    }

    /// One call that may change something, or a read the policy refuses:
    /// the policy decides whether it runs; a bash write the boundary
    /// blocked says so (D-23); a checkpoint follows.
    async fn run_one(
        &mut self,
        id: &str,
        callable: &Tool,
        args: &serde_json::Map<String, Value>,
    ) -> Result<ToolOutput, String> {
        if let Decision::Refuse(msg) = decide(&self.safety.policy, callable, args, &self.ctx) {
            self.policy(id, "refused", &msg)?;
            return Ok(ToolOutput::err(msg));
        }
        let mut out = callable.run(args, &self.ctx).await;
        if matches!(callable, Tool::Bash)
            && out.is_error
            && let Some(sb) = self.ctx.sandbox.clone()
            && let Some(path) = sb.blocked(&out.text)
        {
            self.policy(id, "blocked", &path.display().to_string())?;
            out.text.push_str(&format!("\n{}", blocked_message(&path)));
        }
        if self.capture("tool", Some(id)).await? {
            self.turn_changed = true;
        }
        Ok(out)
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
    /// /undo can find it; a tool step only when files changed or the
    /// project's branch tips moved. Returns whether files changed. A failed
    /// capture is recorded and the run goes on.
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
                tracing::debug!(
                    ms = c.elapsed.as_millis() as u64,
                    changed = c.changed,
                    "checkpoint"
                );
                if c.changed || c.refs_moved || trigger == "submit" {
                    self.record(EntryBody::Checkpoint {
                        commit: c.commit,
                        trigger: Some(trigger.into()),
                        tool_call_id: call.map(str::to_owned),
                    })?;
                }
                Ok(c.changed)
            }
            Err(e) => {
                self.record(EntryBody::GuardEvent {
                    guard: "checkpoint_failed".into(),
                    message: format!(
                        "the checkpoint failed, so changes since the last one can't be undone: {e}"
                    ),
                    tool_call_id: call.map(str::to_owned),
                })?;
                Ok(false)
            }
        }
    }

    /// Context overflow (§4.2): drop the oldest turns so the request fits,
    /// and record it.
    async fn drop_old_turns(&mut self, n_prompt: u64, n_ctx: u64) -> Result<(), String> {
        let keep = plan_drop(&self.session.branch(), n_prompt, n_ctx, self.cfg.max_output)
            .ok_or_else(|| format!("the request ({n_prompt} tokens) does not fit the context window ({n_ctx} tokens), even with only the last turn"))?;
        self.record(EntryBody::Compaction {
            summary: String::new(),
            first_kept_id: keep,
            tokens_before: Some(n_prompt),
            replaces: vec![],
        })?;
        self.record(EntryBody::GuardEvent {
            guard: "context_overflow".into(),
            message: format!("the request was {n_prompt} tokens for a {n_ctx}-token window; dropped the oldest turns"),
            tool_call_id: None,
        })?;
        Ok(())
    }
}
