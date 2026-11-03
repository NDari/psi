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
use psi_safety::blocked::{blocked_message, dir_to_add};
use psi_safety::checkpoint::Checkpoints;
use psi_safety::policy::{Decision, Mode, Policy};
use psi_session::{Block, Entry, EntryBody, Role, SessionFile, StopReason, Usage};
use psi_tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::approval::{
    Approval, ApprovalRequest, Approver, BlockedChoice, BlockedRequest, Headless,
};
use crate::compact::CompactionSettings;
use crate::events::AgentEvent;
use crate::ext::{Callable, decide, invoke, schema_of, tool_runner};
use crate::guards::{GuardAction, Guards};
use crate::messages::{build_messages, plan_drop};
use crate::plan::{self, PlanState};
use crate::post_edit::{self, PostEditCheck};
use crate::subagent::{self, AgentDef};
use psi_ext::{AgentRequest, ExtManager, Gate, ToolInfo};
use psi_mcp::{McpManager, McpTool};

mod fit;
mod sub;

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
    ext: Arc<ExtManager>,
    /// Extension tools and their schemas.
    ext_tools: Vec<(ToolInfo, ToolSchema)>,
    /// set_active_tools: the tool names offered, when an extension chose.
    active: Option<Vec<String>>,
    /// before_agent_start: text added to the system prompt for this run.
    run_system: String,
    /// Run after a turn whose write or edit calls changed files (D-36).
    post_edit: Option<PostEditCheck>,
    compaction: CompactionSettings,
    /// Plan mode (D-34).
    plan: PlanState,
    /// Plan mode was turned off by hand: say so with the next prompt.
    plan_off_note: bool,
    /// The plan state changed before the session had its first entry.
    plan_unrecorded: bool,
    /// Sub-agents the `agent` tool offers (§10.1); empty: no tool.
    subagents: Vec<AgentDef>,
    /// This agent is a sub-agent with this name: one level only, and only
    /// tool events reach extensions.
    sub: Option<String>,
    /// A sub-agent without write or edit: bash runs read-only commands and
    /// extension tools that may change things are refused.
    reads_only: bool,
    /// The last request sent, to check a summary request extends it (D-38).
    last_request: Option<ChatRequest>,
    /// The context size when summarizing last failed: no new try until it
    /// grows by a tenth of the window.
    compact_failed_at: Option<u64>,
    /// MCP servers (§10.2), and their tools as taken at the start of the
    /// run: the list never changes inside a run (D-42).
    mcp: Arc<McpManager>,
    mcp_tools: Vec<Arc<McpTool>>,
    /// The first run waited for the servers to connect.
    mcp_waited: bool,
    /// The grants given at the start (`--allow`); `s` answers come on top
    /// and last until the session changes.
    base_grants: HashSet<String>,
    /// `s` answers to extension commands (psi.exec), until the session changes.
    ext_grants: Arc<Mutex<HashSet<String>>>,
}

/// A call after the shape-fix pipeline: the tool and its arguments, or the
/// refusal sent back instead.
type Planned = Result<(Callable, serde_json::Map<String, Value>), String>;

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

/// The extension events a sub-agent sends (§10.1).
const SUB_EVENTS: [&str; 5] = [
    "tool_call",
    "tool_execution_update",
    "tool_result",
    "policy_decision",
    "guard_event",
];

/// How long the first run waits for MCP servers still connecting.
const MCP_WAIT: Duration = Duration::from_secs(10);

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
            ext: ExtManager::empty(),
            ext_tools: Vec::new(),
            active: None,
            run_system: String::new(),
            post_edit: None,
            compaction: CompactionSettings::default(),
            plan: PlanState::default(),
            plan_off_note: false,
            plan_unrecorded: false,
            subagents: Vec::new(),
            sub: None,
            reads_only: false,
            last_request: None,
            compact_failed_at: None,
            mcp: McpManager::empty(),
            mcp_tools: Vec::new(),
            mcp_waited: false,
            base_grants: HashSet::new(),
            ext_grants: Arc::default(),
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

    /// Extensions: their tools join the core ones and their handlers see
    /// the run's events (§9.4).
    pub fn with_extensions(mut self, ext: Arc<ExtManager>) -> Self {
        self.ext = ext;
        self.refresh_extensions();
        self
    }

    pub fn extensions(&self) -> &Arc<ExtManager> {
        &self.ext
    }

    /// After /reload: the extension tools again, and the runner for
    /// `psi.execute_tool`.
    pub fn refresh_extensions(&mut self) {
        let core: Vec<&str> = self.cfg.tools.iter().map(|t| t.name()).collect();
        self.ext_tools = self
            .ext
            .tools()
            .into_iter()
            .filter(|t| !core.contains(&t.name.as_str()))
            .map(|t| {
                let s = schema_of(&t);
                (t, s)
            })
            .collect();
        // The runner and shared state are the main agent's.
        if self.sub.is_some() {
            return;
        }
        self.set_tool_runner();
        let names = self.tool_names();
        self.ext.update_shared(|s| s.active_tools = names);
    }

    /// `psi.execute_tool` runs with this agent's current cancel token and
    /// policy: set again at each run, after an abort renewed the token or
    /// `/mode` changed the policy.
    fn set_tool_runner(&self) {
        if self.sub.is_some() {
            return;
        }
        self.ext.set_tool_runner(tool_runner(
            self.ctx.clone(),
            self.safety.policy.clone(),
            self.safety.approver.clone(),
            self.cfg.tools.clone(),
            Arc::downgrade(&self.ext),
        ));
        self.ext
            .set_services(Arc::new(crate::services::ExtServices {
                ctx: self.ctx.clone(),
                policy: self.safety.policy.clone(),
                approver: self.safety.approver.clone(),
                grants: self.safety.grants.clone(),
                ext_grants: self.ext_grants.clone(),
                ext: Arc::downgrade(&self.ext),
            }));
    }

    /// The tools offered to the model, core first. Plan mode leaves out
    /// write and edit (pi parity).
    fn tool_names(&self) -> Vec<String> {
        let planning = self.plan.enabled;
        let all = self
            .cfg
            .tools
            .iter()
            .filter(|t| !(planning && matches!(t, Tool::Write | Tool::Edit)))
            .map(|t| t.name().to_owned())
            .chain(self.ext_tools.iter().map(|(t, _)| t.name.clone()))
            .chain(self.offers_agents().then(|| subagent::TOOL.to_owned()))
            .chain(self.mcp_tools.iter().map(|t| t.name.clone()));
        match &self.active {
            Some(a) => all.filter(|n| a.contains(n)).collect(),
            None => all.collect(),
        }
    }

    /// Every schema the shape fixes may resolve to, offered or not.
    fn all_schemas(&self) -> Vec<ToolSchema> {
        let mut v = self.schemas.clone();
        v.extend(self.ext_tools.iter().map(|(_, s)| s.clone()));
        if self.offers_agents() {
            v.push(subagent::schema(&self.subagents));
        }
        v.extend(self.mcp_tools.iter().map(|t| t.schema.clone()));
        v
    }

    /// The `agent` tool: sub-agents are set, and this isn't one (D-41).
    fn offers_agents(&self) -> bool {
        !self.subagents.is_empty() && self.sub.is_none()
    }

    /// MCP servers (§10.2): their tools join from the next run.
    pub fn with_mcp(mut self, mcp: Arc<McpManager>) -> Self {
        self.mcp = mcp;
        self.mcp_waited = false;
        self
    }

    pub fn mcp(&self) -> &Arc<McpManager> {
        &self.mcp
    }

    /// The MCP tools for this run: the first run waits up to
    /// [`MCP_WAIT`] for servers still connecting; a server that connects
    /// later joins at the next run.
    async fn refresh_mcp(&mut self) {
        if self.mcp.is_empty() {
            return;
        }
        let first = !self.mcp_waited;
        if first {
            self.mcp_waited = true;
            self.mcp.wait(MCP_WAIT).await;
        }
        let core: Vec<&str> = self.cfg.tools.iter().map(|t| t.name()).collect();
        let mut taken: Vec<&str> = core;
        taken.extend(self.ext_tools.iter().map(|(t, _)| t.name.as_str()));
        taken.push(subagent::TOOL);
        self.mcp_tools = self.mcp.tools(&taken);
        self.refresh_tool_names();
        // Once, and only from the main agent.
        if first && self.sub.is_none() {
            for text in psi_mcp::notices(&self.mcp.status(&taken), self.cfg.n_ctx) {
                self.emit(AgentEvent::Notice { text });
            }
        }
    }

    /// Sub-agents for the `agent` tool (§10.1).
    pub fn with_subagents(mut self, defs: Vec<AgentDef>) -> Self {
        self.subagents = defs;
        self.refresh_tool_names();
        self
    }

    /// What extensions asked for since the last step.
    fn apply_ext_requests(&mut self) -> Result<(), String> {
        // Requests are for the main agent; a sub-agent leaves them.
        if self.sub.is_some() {
            return Ok(());
        }
        for r in self.ext.take_agent_requests() {
            match r {
                AgentRequest::SendMessage { custom_type, text } => {
                    self.add_context(&custom_type, &text)?
                }
                AgentRequest::AppendEntry { custom_type, data } => {
                    self.record(EntryBody::Custom { custom_type, data })?;
                }
                AgentRequest::SetActiveTools(names) => {
                    self.active = Some(names);
                    let names = self.tool_names();
                    self.ext.update_shared(|s| s.active_tools = names);
                }
            }
        }
        Ok(())
    }

    /// The branch's custom entries and the session file, for extensions.
    fn share_session(&self) {
        if self.sub.is_some() {
            return;
        }
        let entries: Vec<(String, Value)> = self
            .session
            .branch()
            .iter()
            .filter_map(|e| match &e.body {
                EntryBody::Custom { custom_type, data } => {
                    Some((custom_type.clone(), data.clone()))
                }
                _ => None,
            })
            .collect();
        let file = self.session.path().display().to_string();
        self.ext.update_shared(|s| {
            s.entries = entries;
            s.session_file = Some(file);
        });
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
        // "Allow for this session" means this session: /new, /resume and
        // /fork drop the `s` answers (always-ask entries) and keep only
        // what --allow granted at the start.
        self.safety.grants = self.base_grants.clone();
        if let Ok(mut g) = self.ext_grants.lock() {
            g.clear();
        }
        self.capture_failing = false;
        self.restore_plan();
        if let Ok(mut f) = self.ctx.files.lock() {
            *f = psi_tools::FileTracker::default();
        }
    }

    pub fn plan(&self) -> &PlanState {
        &self.plan
    }

    /// Turns plan mode on or off (/plan, --plan). Turning it on drops a plan
    /// being carried out; turning it off tells the model with the next prompt.
    pub fn set_plan_mode(&mut self, on: bool) -> Result<(), String> {
        if self.plan.enabled == on {
            return Ok(());
        }
        self.plan_off_note = !on;
        self.plan.enabled = on;
        if on {
            self.plan.executing = false;
            self.plan.steps.clear();
        }
        self.refresh_tool_names();
        self.record_plan()
    }

    /// Leaves plan mode to carry out the proposed plan; returns the prompt
    /// that starts it.
    pub fn start_plan(&mut self) -> Result<String, String> {
        if self.plan.steps.is_empty() {
            return Err("there is no plan to carry out".into());
        }
        self.plan.enabled = false;
        self.plan.executing = true;
        self.plan_off_note = false;
        for s in &mut self.plan.steps {
            s.done = false;
        }
        self.refresh_tool_names();
        self.record_plan()?;
        Ok(plan::execute_prompt(&self.plan.steps))
    }

    /// Plan mode as the current branch last left it (/resume, -c).
    pub fn restore_plan(&mut self) {
        self.plan = self
            .session
            .branch()
            .iter()
            .rev()
            .find_map(|e| match &e.body {
                EntryBody::Custom { custom_type, data } if custom_type == "plan" => {
                    Some(PlanState::from_json(data))
                }
                _ => None,
            })
            .unwrap_or_default();
        self.refresh_tool_names();
    }

    fn record_plan(&mut self) -> Result<(), String> {
        // session_info stays the first entry; the run records this after it.
        if self.session.entries().is_empty() {
            self.plan_unrecorded = true;
            return Ok(());
        }
        self.plan_unrecorded = false;
        let data = self.plan.to_json();
        self.record(EntryBody::Custom {
            custom_type: "plan".into(),
            data,
        })
        .map(|_| ())
    }

    fn refresh_tool_names(&self) {
        if self.sub.is_some() {
            return;
        }
        let names = self.tool_names();
        self.ext.update_shared(|s| s.active_tools = names);
    }

    /// The tail size and summary form compaction uses (the `compaction` setting).
    pub fn with_compaction(mut self, settings: CompactionSettings) -> Self {
        self.compaction = settings;
        self
    }

    /// The project's check command, run after turns that edited files.
    pub fn with_post_edit_check(mut self, check: Option<PostEditCheck>) -> Self {
        self.post_edit = check;
        self
    }

    pub fn with_safety(mut self, safety: Safety) -> Self {
        self.base_grants = safety.grants.clone();
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

    /// Whether extensions see this event: a sub-agent sends only its tool
    /// events (§10.1), so handlers of the run's lifecycle see one run.
    fn ext_sees(&self, event: &str) -> bool {
        self.sub.is_none() || SUB_EVENTS.contains(&event)
    }

    /// A sub-agent's event payload carries its name as `agent`.
    fn tag(&self, mut v: Value) -> Value {
        if let (Some(a), Value::Object(m)) = (&self.sub, &mut v) {
            m.insert("agent".into(), Value::String(a.clone()));
        }
        v
    }

    fn ext_listens(&self, event: &str) -> bool {
        self.ext_sees(event) && self.ext.listens(event)
    }

    async fn ext_notify(&self, event: &str, payload: Value) {
        if self.ext_sees(event) {
            self.ext.notify(event, self.tag(payload)).await;
        }
    }

    async fn ext_gate(&self, event: &str, payload: Value) -> Gate {
        if self.ext_sees(event) {
            self.ext.gate(event, self.tag(payload)).await
        } else {
            Gate::Allow(payload)
        }
    }

    async fn ext_chain(&self, event: &str, payload: Value) -> Value {
        if self.ext_sees(event) {
            self.ext.chain(event, self.tag(payload)).await
        } else {
            payload
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
        let ev = match &e.body {
            EntryBody::PolicyDecision {
                tool_call_id,
                decision,
                reason,
                ..
            } => Some((
                "policy_decision",
                json!({"tool_call_id": tool_call_id, "decision": decision, "reason": reason}),
            )),
            EntryBody::GuardEvent { guard, message, .. } => {
                Some(("guard_event", json!({"guard": guard, "message": message})))
            }
            _ => None,
        };
        if let Some((name, payload)) = ev
            && self.ext_listens(name)
        {
            let ext = self.ext.clone();
            let payload = self.tag(payload);
            tokio::spawn(async move { ext.notify(name, payload).await });
        }
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
        self.set_tool_runner();
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
        if self.plan.enabled && out.stop == Stop::Done {
            let steps = plan::extract_steps(&out.text);
            if !steps.is_empty() {
                self.plan.steps = steps
                    .into_iter()
                    .map(|text| plan::Step { text, done: false })
                    .collect();
                if let Err(e) = self.record_plan() {
                    out.stop = Stop::Failed(e);
                }
            }
        }
        self.run_system.clear();
        self.ext_notify(
            "agent_end",
            json!({"stop": out.stop.name(), "text": out.text}),
        )
        .await;
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
        self.refresh_mcp().await;
        if self.session.entries().is_empty() {
            self.record(EntryBody::Custom {
                custom_type: "session_info".into(),
                data: json!({"agent": "psi", "version": env!("CARGO_PKG_VERSION"),
                             "cwd": self.ctx.cwd, "model": self.cfg.model,
                             "safety": self.safety.info}),
            })?;
        }
        if self.plan_unrecorded {
            self.record_plan()?;
        }
        // At submit: this also captures the user's own edits (§7.2).
        self.capture("submit", None).await?;
        self.restore_todos();
        self.record(Self::user_message(input))?;
        if self.plan.enabled {
            self.add_context("plan_mode", plan::PLAN_CONTEXT)?;
        } else if std::mem::take(&mut self.plan_off_note) {
            self.add_context("plan_mode", plan::PLAN_OFF)?;
        }
        self.share_session();
        self.apply_ext_requests()?;
        let start_ev = json!({"prompt": input, "system": ""});
        let r = self.ext_chain("before_agent_start", start_ev).await;
        self.run_system = r["system"].as_str().unwrap_or("").to_owned();
        self.ext_notify("agent_start", json!({"prompt": input}))
            .await;
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
            self.apply_ext_requests()?;
            self.emit(AgentEvent::TurnStart { turn });
            self.ext_notify("turn_start", json!({"turn": turn})).await;

            if self.fit_context().await?
                && let Some(GuardAction::Pause { guard, reason }) = guards.on_compaction()
            {
                self.record(EntryBody::GuardEvent {
                    guard: guard.into(),
                    message: format!("paused: {reason}"),
                    tool_call_id: None,
                })?;
                if let Some(stop) = self.pause_run(reason).await {
                    return Ok(stop);
                }
                guards.reset();
            }
            let resp = match self.request(turn).await {
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
            if self.plan.executing && self.plan.mark_done(&resp.text) {
                if self.plan.done_count() == self.plan.steps.len() {
                    self.plan.executing = false;
                }
                self.record_plan()?;
            }
            if self.ext_listens("message_end") {
                let calls: Vec<Value> = resp
                    .tool_calls
                    .iter()
                    .map(|c| json!({"id": c.id, "name": c.name, "arguments": c.arguments}))
                    .collect();
                self.ext_notify(
                    "message_end",
                    json!({"text": resp.text, "tool_calls": calls}),
                )
                .await;
            }

            if resp.tool_calls.is_empty() {
                self.emit(AgentEvent::TurnEnd { turn });
                self.ext_notify("turn_end", json!({"turn": turn})).await;
                // Steering typed during the answer goes in now, as after a
                // tool turn (pi parity); before, it waited and slipped into
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

            let allowed = self
                .cfg
                .budgets
                .max_tool_calls
                .saturating_sub(out.tool_calls) as usize;
            let (n, pause) = self.run_calls(&resp, &mut guards, allowed).await?;
            out.tool_calls += n;
            self.emit(AgentEvent::TurnEnd { turn });
            self.ext_notify("turn_end", json!({"turn": turn})).await;
            if self.ctx.cancel.is_cancelled() {
                return Ok(Stop::Aborted);
            }
            if let Some(reason) = pause {
                if let Some(stop) = self.pause_run(reason).await {
                    return Ok(stop);
                }
                guards.reset();
            }
            for s in Queues::take(&self.queues.steering) {
                self.record(Self::user_message(&s))?;
            }
        }
    }

    /// Asks whether to go on after a guard's pause; the stop when not.
    async fn pause_run(&self, reason: String) -> Option<Stop> {
        let go_on = tokio::select! {
            g = self.safety.approver.pause(reason.clone()) => g,
            () = self.ctx.cancel.cancelled() => return Some(Stop::Aborted),
        };
        (!go_on).then_some(Stop::Guard(reason))
    }

    fn system_prompt(&self) -> String {
        if self.run_system.is_empty() {
            self.cfg.system_prompt.clone()
        } else {
            format!("{}\n\n{}", self.cfg.system_prompt, self.run_system)
        }
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

    async fn request(&mut self, turn: u32) -> Result<Response, AiError> {
        let req = self.build_request();
        self.last_request = Some(req.clone());
        if self.ext_listens("context") {
            let m: Vec<Value> = req.messages.iter().map(|m| m.to_json()).collect();
            self.ext_notify("context", json!({"messages": m})).await;
        }
        self.ext_notify("message_start", json!({"turn": turn}))
            .await;
        // Streamed text reaches extensions in order, through one task.
        let updates = self.ext_listens("message_update").then(|| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let ext = self.ext.clone();
            tokio::spawn(async move {
                while let Some(t) = rx.recv().await {
                    ext.notify("message_update", json!({"text": t})).await;
                }
            });
            tx
        });
        let events = self.events.clone();
        let mut on_delta = move |d: Delta| {
            if let Delta::Text(t) = d {
                if let Some(u) = &updates {
                    let _ = u.send(t.clone());
                }
                if let Some(tx) = &events {
                    let _ = tx.send(AgentEvent::TextDelta { text: t });
                }
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
        allowed: usize,
    ) -> Result<(u32, Option<String>), String> {
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
                    let core = Tool::from_name(&f.name)
                        .filter(|t| self.cfg.tools.contains(t))
                        .map(Callable::Core);
                    let ext = || {
                        self.ext_tools
                            .iter()
                            .find(|(t, _)| t.name == f.name)
                            .map(|(t, _)| Callable::Ext(t.clone()))
                    };
                    let agent = || {
                        (f.name == subagent::TOOL && self.offers_agents())
                            .then_some(Callable::Agent)
                    };
                    let mcp = || {
                        self.mcp_tools
                            .iter()
                            .find(|t| t.name == f.name)
                            .map(|t| Callable::Mcp(t.clone(), self.mcp.clone()))
                    };
                    match core
                        .or_else(ext)
                        .or_else(agent)
                        .or_else(mcp)
                        .filter(|c| offered.iter().any(|n| n == c.name()))
                    {
                        Some(c) => Ok((c, f.args)),
                        None if self.plan.enabled
                            && matches!(f.name.as_str(), "write" | "edit") =>
                        {
                            Err(format!(
                                "{} is off in plan mode. Put the change in your plan instead.",
                                f.name
                            ))
                        }
                        None => Err(format!("There is no tool named '{}'.", f.name)),
                    }
                }
                Err(msg) => Err(msg),
            };
            // Past the budget: not run, though the response asked for it.
            let fixed = if i >= allowed {
                Err(format!(
                    "Not run: this run reached its limit of {} tool calls. Say in your answer what is left to do.",
                    self.cfg.budgets.max_tool_calls
                ))
            } else {
                fixed
            };
            planned.push((call.clone(), fixed));
        }

        // tool_call: extensions may block a call or rewrite its arguments,
        // before the policy sees it (§9.4); both are recorded.
        if self.ext_listens("tool_call") {
            for (call, p) in planned.iter_mut() {
                let Ok((c, args)) = p else { continue };
                let ev =
                    json!({"id": call.id, "name": c.name(), "args": Value::Object(args.clone())});
                match self.ext_gate("tool_call", ev).await {
                    Gate::Block(reason) => {
                        self.policy(&call.id, "blocked_by_extension", &reason)?;
                        *p = Err(format!("Blocked: {reason}"));
                    }
                    Gate::Allow(ev) => {
                        if let Some(Value::Object(new)) = ev.get("args")
                            && new != args
                        {
                            self.record(EntryBody::PolicyDecision {
                                tool_call_id: call.id.clone(),
                                decision: "rewritten".into(),
                                reason: "an extension rewrote the arguments".into(),
                                rewrite: Some(Value::Object(new.clone())),
                            })?;
                            *args = new.clone();
                        }
                    }
                }
            }
        }

        self.turn_changed = false;
        let mut outputs: Vec<Option<ToolOutput>> = vec![None; planned.len()];
        let mut i = 0;
        while i < planned.len() {
            // A batch of read-only calls, or one other call.
            let mut j = i + 1;
            if matches!(&planned[i].1, Ok((c, _)) if c.read_only()) {
                while j < planned.len() && matches!(&planned[j].1, Ok((c, _)) if c.read_only()) {
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
                    let ev = json!({"id": call.id, "name": c.name(), "args": Value::Object(args.clone())});
                    self.ext_notify("tool_execution_update", ev).await;
                }
            }
            if let (call, Ok((c, args))) = &planned[i]
                && !c.read_only()
            {
                outputs[i] = Some(self.run_one(&call.id, c, args, &resp.text).await?);
                i = j;
                continue;
            }
            let (ctx, ext) = (&self.ctx, &*self.ext);
            let futs = planned[i..j].iter().map(|(_, p)| async move {
                match p {
                    Ok((c, args)) => invoke(c, args, ctx, ext).await,
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
        // tool_result: extensions may change what the model sees.
        if self.ext_listens("tool_result") {
            for ((call, p), o) in planned.iter().zip(outputs.iter_mut()) {
                let Ok((c, args)) = p else { continue };
                let ev = json!({"id": call.id, "name": c.name(), "args": Value::Object(args.clone()),
                                "text": o.text, "is_error": o.is_error});
                let r = self.ext_chain("tool_result", ev).await;
                if let Some(t) = r["text"].as_str() {
                    o.text = t.to_owned();
                }
                if let Some(e) = r["is_error"].as_bool() {
                    o.is_error = e;
                }
            }
        }
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
        let wrote = planned.iter().zip(&outputs).any(|((_, p), o)| {
            matches!(p, Ok((Callable::Core(Tool::Write | Tool::Edit), _))) && !o.is_error
        });
        if let Some(action) = guards.on_turn_end(self.turn_changed || wrote)
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
        let last = planned.iter().zip(outputs.iter()).rposition(|((_, p), o)| {
            !o.is_error && matches!(p, Ok((Callable::Core(Tool::Write | Tool::Edit), _)))
        });
        let Some(i) = last else { return Ok(()) };
        if self.ctx.cancel.is_cancelled() {
            return Ok(());
        }
        let args = json!({"command": check.command, "timeout": check.timeout_s});
        let Value::Object(args) = args else {
            return Ok(());
        };
        let started = Instant::now();
        let out = psi_tools::bash::run(args, &self.ctx, false).await;
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

    /// One call that may change something: the policy decides whether it
    /// runs, asks or is refused; a blocked bash write offers the user a
    /// rerun (D-23); a checkpoint follows.
    async fn run_one(
        &mut self,
        id: &str,
        callable: &Callable,
        args: &serde_json::Map<String, Value>,
        preceding: &str,
    ) -> Result<ToolOutput, String> {
        if (self.plan.enabled || self.reads_only)
            && let Some(why) = self.plan_refusal(callable, args)
        {
            self.policy(id, "refused", &why)?;
            return Ok(ToolOutput::err(why));
        }
        if matches!(callable, Callable::Agent) {
            return self.run_subagent(id, args).await;
        }
        let decision = decide(
            &self.safety.policy,
            callable,
            args,
            &self.ctx,
            self.safety.checkpoints.clone(),
            self.capture_failing,
            &self.ext,
        )
        .await;
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
                        tool: callable.name().into(),
                        args: Value::Object(args.clone()),
                        intent: args
                            .get("intent")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        preceding_text: preceding.into(),
                        reason: reason.clone(),
                        keys: keys.clone(),
                        agent: self.sub.clone(),
                    };
                    let answer = tokio::select! {
                        a = self.safety.approver.approve(req) => a,
                        () = self.ctx.cancel.cancelled() => {
                            // Esc while the card was up: stopped, not denied,
                            // so the model isn't told the user said no.
                            self.policy(id, "aborted", &reason)?;
                            return Ok(ToolOutput::err(
                                "Not run: the user stopped the run before answering whether to run it.",
                            ));
                        }
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

        let mut out = invoke(callable, args, &self.ctx, &self.ext).await;
        if matches!(callable, Callable::Core(Tool::Todo)) {
            // Only the list changed: keep a snapshot, no checkpoint.
            let snap = self
                .ctx
                .todos
                .lock()
                .map(|l| l.to_json())
                .unwrap_or_default();
            self.record(EntryBody::Custom {
                custom_type: "todo".into(),
                data: snap,
            })?;
            return Ok(out);
        }
        if matches!(callable, Callable::Core(Tool::Bash))
            && out.is_error
            && let Some(sb) = self.ctx.sandbox.clone()
            && let Some(path) = sb.blocked(&out.text)
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
                        let again = invoke(callable, args, &self.ctx, &self.ext).await;
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

    /// Why plan mode doesn't run a call: bash that isn't read-only, or an
    /// extension tool that may change things.
    /// A read-only sub-agent gets the same refusals, in its own words.
    fn plan_refusal(
        &self,
        callable: &Callable,
        args: &serde_json::Map<String, Value>,
    ) -> Option<String> {
        let (who, instead) = match (&self.sub, self.plan.enabled) {
            (Some(name), false) => (
                format!("{name} is a read-only agent"),
                "Say in your answer what should change instead.",
            ),
            _ => (
                "plan mode allows only reading".to_owned(),
                "Put it in your plan instead.",
            ),
        };
        match callable {
            Callable::Core(Tool::Bash) => {
                let cmd = args.get("command").and_then(Value::as_str).unwrap_or("");
                psi_safety::read_only::check(cmd)
                    .err()
                    .map(|why| format!("Not run: {who}, and {why}. {instead}"))
            }
            Callable::Ext(i) if !i.read_only => Some(format!(
                "Not run: {} may change things, and {who}. {instead}",
                i.name
            )),
            Callable::Mcp(t, _) if !t.read_only => Some(format!(
                "Not run: {} may change things (its server doesn't mark it read-only), and {who}. {instead}",
                t.name
            )),
            Callable::Agent => {
                let name = args.get("agent").and_then(Value::as_str).unwrap_or("");
                let ro = self
                    .subagents
                    .iter()
                    .find(|d| d.name == name)
                    .is_none_or(|d| d.read_only(&self.cfg.tools));
                (!ro).then(|| format!("Not run: {name} may change files, and {who}. Use a read-only agent such as scout, or {}", instead.to_lowercase()))
            }
            _ => None,
        }
    }

    /// The todo list as the current branch last left it, so it follows
    /// /tree, /rewind and /resume.
    fn restore_todos(&mut self) {
        let list = self
            .session
            .branch()
            .iter()
            .rev()
            .find_map(|e| match &e.body {
                EntryBody::Custom { custom_type, data } if custom_type == "todo" => {
                    Some(psi_tools::TodoList::from_json(data))
                }
                _ => None,
            })
            .unwrap_or_default();
        if let Ok(mut l) = self.ctx.todos.lock() {
            *l = list;
        }
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
    async fn drop_old_turns(&mut self, n_prompt: u64, n_ctx: u64) -> Result<(), String> {
        if let Gate::Block(why) = self
            .ext
            .gate(
                "session_before_compact",
                json!({"tokens": n_prompt, "reason": "overflow"}),
            )
            .await
        {
            return Err(format!(
                "the request ({n_prompt} tokens) does not fit the context window ({n_ctx} tokens), and an extension stopped dropping old turns: {why}"
            ));
        }
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
