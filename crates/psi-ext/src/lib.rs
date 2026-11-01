//! The extension host (psi-plan.md §9): Lua extensions, one actor each
//! (D-13), and script tools (D-22). All extensions are trusted (D-25);
//! limits catch bugs, not attacks.

pub mod actor;
pub mod check;
pub mod host;
pub mod limits;
pub mod load;
pub mod testing;

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::{Value as Json, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

pub use actor::{Actor, EventOut, Kind};
pub use host::{ProviderInfo, ProviderModel, Registrations, Request, Shared, ToolInfo};
pub use limits::Limits;
pub use load::{ScriptTool, Source};

use actor::Msg;

/// Runs a tool for `psi.execute_tool`, through the policy.
pub type ToolRunner = Arc<dyn Fn(String, Json) -> BoxFuture<'static, (String, bool)> + Send + Sync>;

/// How long psi waits for one extension's answer to an event, at most.
/// A runaway handler is stopped sooner, by the busy limit.
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);

/// What extensions asked of the agent, applied between steps.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentRequest {
    SendMessage { custom_type: String, text: String },
    AppendEntry { custom_type: String, data: Json },
    SetActiveTools(Vec<String>),
}

/// The outcome of a gate event.
#[derive(Debug, Clone, PartialEq)]
pub enum Gate {
    /// Go on, with the payload as the handlers left it (rewritten args).
    Allow(Json),
    Block(String),
}

pub struct ExtManager {
    actors: RwLock<Vec<Arc<Actor>>>,
    scripts: RwLock<Vec<ScriptTool>>,
    pub shared: Arc<RwLock<Shared>>,
    agent: Mutex<VecDeque<AgentRequest>>,
    runner: RwLock<Option<ToolRunner>>,
    /// UI requests: notify, confirm, select, input, status, widget, custom,
    /// send_user_message.
    ui: UnboundedSender<(String, Request)>,
    requests: UnboundedSender<(String, Request)>,
    limits: Limits,
    /// Extensions are loading: nothing can answer the user's questions yet.
    loading: std::sync::atomic::AtomicBool,
    /// UI questions asked during setup, reported with the load errors.
    setup_asks: Mutex<Vec<String>>,
}

/// Which kind an event is (from the API definition).
pub fn kind_of(event: &str) -> Kind {
    match psi_ext_api::EVENTS
        .iter()
        .find(|e| e.name == event)
        .map(|e| e.kind)
    {
        Some("gate") => Kind::Gate,
        Some("chain") => Kind::Chain,
        _ => Kind::Notify,
    }
}

impl ExtManager {
    /// Starts every extension; returns the manager and what failed to load.
    /// Blocks while extensions load: call it from a blocking context.
    pub fn start(
        sources: &[Source],
        shared: Arc<RwLock<Shared>>,
        limits: Limits,
        ui: UnboundedSender<(String, Request)>,
    ) -> (Arc<Self>, Vec<String>) {
        let (tx, rx) = unbounded_channel();
        let m = Arc::new(Self {
            actors: RwLock::new(Vec::new()),
            scripts: RwLock::new(Vec::new()),
            shared,
            agent: Mutex::new(VecDeque::new()),
            runner: RwLock::new(None),
            ui,
            requests: tx,
            limits,
            loading: false.into(),
            setup_asks: Mutex::new(Vec::new()),
        });
        // Routed while loading too: a setup that asks the user gets an
        // answer at once instead of waiting out the load timeout.
        tokio::spawn(route(Arc::downgrade(&m), rx));
        let errors = m.load(sources);
        (m, errors)
    }

    /// No extensions: every call is a no-op.
    pub fn empty() -> Arc<Self> {
        let (ui, _) = unbounded_channel();
        let (requests, _) = unbounded_channel();
        Arc::new(Self {
            actors: RwLock::new(Vec::new()),
            scripts: RwLock::new(Vec::new()),
            shared: Arc::new(RwLock::new(Shared::default())),
            agent: Mutex::new(VecDeque::new()),
            runner: RwLock::new(None),
            ui,
            requests,
            limits: Limits::default(),
            loading: false.into(),
            setup_asks: Mutex::new(Vec::new()),
        })
    }

    fn load(&self, sources: &[Source]) -> Vec<String> {
        self.loading
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut errors = self.load_all(sources);
        self.loading
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut asks) = self.setup_asks.lock() {
            errors.append(&mut asks);
        }
        errors
    }

    fn load_all(&self, sources: &[Source]) -> Vec<String> {
        let mut errors = Vec::new();
        let mut actors = Vec::new();
        let mut scripts: Vec<ScriptTool> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for s in sources {
            match s {
                Source::Lua(path) => match actor::spawn(
                    path,
                    self.shared.clone(),
                    self.requests.clone(),
                    self.limits.clone(),
                ) {
                    Ok(a) => {
                        let dup = a
                            .regs
                            .tools
                            .iter()
                            .find(|t| names.contains(&t.name))
                            .map(|t| t.name.clone());
                        if let Some(d) = dup {
                            errors.push(format!(
                                "{}: tool '{d}' is already registered; extension not loaded",
                                a.name
                            ));
                            let _ = a.tx.send(Msg::Shutdown);
                            continue;
                        }
                        names.extend(a.regs.tools.iter().map(|t| t.name.clone()));
                        actors.push(Arc::new(a));
                    }
                    Err(e) => errors.push(e),
                },
                Source::Script(t) => {
                    if names.contains(&t.name) {
                        errors.push(format!(
                            "{}: tool '{}' is already registered",
                            t.dir.display(),
                            t.name
                        ));
                        continue;
                    }
                    names.push(t.name.clone());
                    scripts.push(t.clone());
                }
            }
        }
        if let Ok(mut a) = self.actors.write() {
            *a = actors;
        }
        if let Ok(mut s) = self.scripts.write() {
            *s = scripts;
        }
        errors
    }

    /// /reload: stops every extension and loads them again.
    pub fn reload(&self, sources: &[Source]) -> Vec<String> {
        self.shutdown();
        self.load(sources)
    }

    pub fn shutdown(&self) {
        for a in self.actors() {
            let _ = a.tx.send(Msg::Shutdown);
        }
    }

    pub fn set_tool_runner(&self, r: ToolRunner) {
        if let Ok(mut w) = self.runner.write() {
            *w = Some(r);
        }
    }

    pub fn actors(&self) -> Vec<Arc<Actor>> {
        self.actors.read().map(|a| a.clone()).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.actors().is_empty() && self.scripts().is_empty()
    }

    pub fn scripts(&self) -> Vec<ScriptTool> {
        self.scripts.read().map(|s| s.clone()).unwrap_or_default()
    }

    /// What the agent should apply before its next request.
    pub fn take_agent_requests(&self) -> Vec<AgentRequest> {
        self.agent
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn update_shared(&self, f: impl FnOnce(&mut Shared)) {
        if let Ok(mut s) = self.shared.write() {
            f(&mut s);
        }
    }

    pub fn listens(&self, event: &str) -> bool {
        self.actors()
            .iter()
            .any(|a| a.regs.events.iter().any(|e| e == event))
    }

    /// Runs an event through every extension that handles it, in load
    /// order: notify ignores results; chain and gate pass the payload on.
    /// A gate fails closed: a handler that fails, takes too long, or whose
    /// extension has stopped blocks. For other events a failing handler is
    /// skipped (the extension reports its error) and the payload goes on as
    /// the earlier handlers left it, so one extension's error doesn't undo
    /// another's change.
    pub async fn emit(&self, event: &str, payload: Json) -> Result<EventOut, (String, String)> {
        let kind = kind_of(event);
        let mut out = EventOut {
            payload,
            results: Vec::new(),
        };
        for a in self.actors() {
            if !a.regs.events.iter().any(|e| e == event) {
                continue;
            }
            let (tx, rx) = oneshot::channel();
            let msg = Msg::Event {
                name: event.into(),
                kind,
                payload: out.payload.clone(),
                reply: tx,
            };
            let gate = kind == Kind::Gate;
            if a.tx.send(msg).is_err() {
                if gate {
                    return Err((a.name.clone(), "it has stopped".into()));
                }
                continue;
            }
            match tokio::time::timeout(EVENT_TIMEOUT, rx).await {
                Ok(Ok(Ok(o))) => {
                    out.payload = o.payload;
                    let block = o.results.iter().any(|r| r["block"] == Json::Bool(true));
                    out.results.extend(o.results);
                    if kind == Kind::Gate && block {
                        return Ok(out);
                    }
                }
                Ok(Ok(Err(e))) if gate => return Err((a.name.clone(), e)),
                Ok(Ok(Err(_))) => {}
                Ok(Err(_)) if gate => {
                    return Err((a.name.clone(), "it stopped without answering".into()));
                }
                Ok(Err(_)) => {}
                Err(_) if gate => {
                    return Err((
                        a.name.clone(),
                        format!("no answer in {}s", EVENT_TIMEOUT.as_secs()),
                    ));
                }
                Err(_) => {}
            }
        }
        Ok(out)
    }

    /// A notify event; errors were already reported by the extension.
    pub async fn notify(&self, event: &str, payload: Json) {
        if self.listens(event) {
            let _ = self.emit(event, payload).await;
        }
    }

    /// A gate event: blocked, or allowed with the (possibly rewritten) payload.
    pub async fn gate(&self, event: &str, payload: Json) -> Gate {
        if !self.listens(event) {
            return Gate::Allow(payload);
        }
        match self.emit(event, payload).await {
            Ok(o) => match o.results.iter().find(|r| r["block"] == Json::Bool(true)) {
                Some(r) => Gate::Block(
                    r["reason"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| "an extension blocked it".into()),
                ),
                None => Gate::Allow(o.payload),
            },
            Err((ext, e)) => Gate::Block(format!(
                "the {ext} extension failed while checking it ({e})"
            )),
        }
    }

    /// A chain event: the payload as the handlers left it.
    pub async fn chain(&self, event: &str, payload: Json) -> Json {
        if !self.listens(event) {
            return payload;
        }
        match self.emit(event, payload.clone()).await {
            Ok(o) => o.payload,
            Err(_) => payload,
        }
    }

    /// Every extension tool: Lua tools, then script tools.
    pub fn tools(&self) -> Vec<ToolInfo> {
        let mut v: Vec<ToolInfo> = self
            .actors()
            .iter()
            .flat_map(|a| a.regs.tools.clone())
            .collect();
        for s in self.scripts() {
            let mut params: Vec<(String, String, String, bool)> = s
                .params
                .iter()
                .map(|(n, p)| {
                    let req = s.required.as_ref().is_none_or(|r| r.contains(n));
                    (
                        n.clone(),
                        p["type"].as_str().unwrap_or("string").to_owned(),
                        p["description"].as_str().unwrap_or("").to_owned(),
                        req,
                    )
                })
                .collect();
            params.sort_by(|a, b| a.0.cmp(&b.0));
            v.push(ToolInfo {
                ext: format!("script:{}", s.name),
                name: s.name.clone(),
                description: s.description.clone(),
                params,
                read_only: s.read_only,
                always_ask: s.always_ask,
            });
        }
        v
    }

    pub fn script(&self, name: &str) -> Option<ScriptTool> {
        self.scripts().into_iter().find(|s| s.name == name)
    }

    /// Runs a Lua tool call.
    pub async fn call_tool(&self, name: &str, args: Json) -> (String, bool) {
        let Some(a) = self
            .actors()
            .into_iter()
            .find(|a| a.regs.tools.iter().any(|t| t.name == name))
        else {
            return (format!("There is no tool named '{name}'."), true);
        };
        let (tx, rx) = oneshot::channel();
        let _ = a.tx.send(Msg::Tool {
            name: name.into(),
            args,
            reply: tx,
        });
        rx.await
            .unwrap_or_else(|_| (format!("The {} extension stopped.", a.name), true))
    }

    /// (name, description, extension)
    pub fn commands(&self) -> Vec<(String, String, String)> {
        self.actors()
            .iter()
            .flat_map(|a| {
                a.regs
                    .commands
                    .iter()
                    .map(|(n, d)| (n.clone(), d.clone(), a.name.clone()))
            })
            .collect()
    }

    pub async fn run_command(&self, name: &str, args: &str) -> Result<(), String> {
        let Some(a) = self
            .actors()
            .into_iter()
            .find(|a| a.regs.commands.iter().any(|(n, _)| n == name))
        else {
            return Err(format!("no command /{name}"));
        };
        let (tx, rx) = oneshot::channel();
        let _ = a.tx.send(Msg::Command {
            name: name.into(),
            args: args.into(),
            reply: tx,
        });
        rx.await
            .unwrap_or_else(|_| Err(format!("the {} extension stopped", a.name)))
    }

    /// Providers from `psi.register_provider`; a later extension's wins.
    pub fn providers(&self) -> Vec<ProviderInfo> {
        let mut out: Vec<ProviderInfo> = Vec::new();
        for p in self.actors().iter().flat_map(|a| a.regs.providers.clone()) {
            out.retain(|q| q.name != p.name);
            out.push(p);
        }
        out
    }

    pub fn shortcuts(&self) -> Vec<(String, String)> {
        self.actors()
            .iter()
            .flat_map(|a| a.regs.shortcuts.clone())
            .collect()
    }

    pub async fn run_shortcut(&self, key: &str) -> Result<(), String> {
        let Some(a) = self
            .actors()
            .into_iter()
            .find(|a| a.regs.shortcuts.iter().any(|(k, _)| k == key))
        else {
            return Err(format!("no shortcut {key}"));
        };
        let (tx, rx) = oneshot::channel();
        let _ = a.tx.send(Msg::Shortcut {
            key: key.into(),
            reply: tx,
        });
        rx.await
            .unwrap_or_else(|_| Err(format!("the {} extension stopped", a.name)))
    }

    /// An extension's lines for a committed tool call, if one renders that
    /// tool and answers within `deadline` (the UI thread waits).
    pub fn render_tool(
        &self,
        tool: &str,
        call: Json,
        width: usize,
        deadline: Duration,
    ) -> Option<Vec<String>> {
        let a = self
            .actors()
            .into_iter()
            .find(|a| a.regs.renderers.iter().any(|r| r == tool))?;
        let (tx, rx) = std::sync::mpsc::channel();
        a.tx.send(Msg::Render {
            tool: tool.into(),
            call,
            width,
            reply: tx,
        })
        .ok()?;
        rx.recv_timeout(deadline).ok().flatten()
    }

    fn actor(&self, ext: &str) -> Option<Arc<Actor>> {
        self.actors().into_iter().find(|a| a.name == ext)
    }

    /// A custom component's lines, or `None` if it didn't answer in time
    /// (the UI keeps the last frame).
    pub fn component_render(
        &self,
        ext: &str,
        id: u64,
        width: usize,
        deadline: Duration,
    ) -> Option<Vec<String>> {
        let a = self.actor(ext)?;
        let (tx, rx) = std::sync::mpsc::channel();
        a.tx.send(Msg::ComponentRender {
            id,
            width,
            reply: tx,
        })
        .ok()?;
        rx.recv_timeout(deadline).ok()
    }

    /// A key for a custom component: `Some(value)` closes it.
    pub fn component_input(
        &self,
        ext: &str,
        id: u64,
        key: &str,
        deadline: Duration,
    ) -> Option<Json> {
        let a = self.actor(ext)?;
        let (tx, rx) = std::sync::mpsc::channel();
        a.tx.send(Msg::ComponentInput {
            id,
            key: key.into(),
            reply: tx,
        })
        .ok()?;
        rx.recv_timeout(deadline).ok().flatten()
    }

    /// For `psi ext` listings and /help.
    pub fn describe(&self) -> Json {
        let exts: Vec<Json> = self
            .actors()
            .iter()
            .map(|a| json!({"name": a.name, "version": a.version, "path": a.path, "registered": a.regs}))
            .collect();
        json!({"extensions": exts, "script_tools": self.scripts().iter().map(|s| &s.name).collect::<Vec<_>>()})
    }
}

/// Routes extension requests: bus events to the other extensions, tool
/// calls to the runner, agent requests to its queue, the rest to the UI.
async fn route(m: Weak<ExtManager>, mut rx: UnboundedReceiver<(String, Request)>) {
    while let Some((ext, req)) = rx.recv().await {
        let Some(m) = m.upgrade() else { break };
        match req {
            Request::Bus { name, data } => {
                for a in m.actors() {
                    let _ = a.tx.send(Msg::Bus {
                        name: name.clone(),
                        data: data.clone(),
                    });
                }
            }
            Request::ExecuteTool { name, args, reply } => {
                let runner = m.runner.read().ok().and_then(|r| r.clone());
                match runner {
                    Some(r) => {
                        tokio::spawn(async move {
                            let _ = reply.send(r(name, args).await);
                        });
                    }
                    None => {
                        let _ = reply.send(("Tools can't run here.".into(), true));
                    }
                }
            }
            Request::SendMessage { custom_type, text } => {
                m.push(AgentRequest::SendMessage { custom_type, text })
            }
            Request::AppendEntry { custom_type, data } => {
                m.push(AgentRequest::AppendEntry { custom_type, data })
            }
            Request::SetActiveTools(names) => m.push(AgentRequest::SetActiveTools(names)),
            Request::Log(msg) => tracing::info!(ext = %ext, "{msg}"),
            // Setup can't ask the user: nothing can answer yet. Empty
            // answers now, and the extension is told where asking works.
            ask @ (Request::Confirm { .. }
            | Request::Select { .. }
            | Request::Input { .. }
            | Request::Custom { .. })
                if m.loading.load(std::sync::atomic::Ordering::SeqCst) =>
            {
                // Recorded before answering, so the load that waits on
                // this setup sees it.
                let what = match &ask {
                    Request::Confirm { .. } => "ui.confirm",
                    Request::Select { .. } => "ui.select",
                    Request::Input { .. } => "ui.input",
                    _ => "ui.custom",
                };
                if let Ok(mut a) = m.setup_asks.lock() {
                    a.push(format!(
                        "extension {ext}: {what} in setup got an empty answer (nothing can ask the user while extensions load); ask from an event, command or tool handler"
                    ));
                }
                match ask {
                    Request::Confirm { reply, .. } => {
                        let _ = reply.send(false);
                    }
                    Request::Select { reply, .. } | Request::Input { reply, .. } => {
                        let _ = reply.send(None);
                    }
                    Request::Custom { reply, .. } => {
                        let _ = reply.send(None);
                    }
                    _ => {}
                }
            }
            other => {
                if let Request::Notify { message, level } = &other {
                    tracing::info!(ext = %ext, level = %level, "{message}");
                }
                let _ = m.ui.send((ext, other));
            }
        }
    }
}

impl ExtManager {
    fn push(&self, r: AgentRequest) {
        if let Ok(mut q) = self.agent.lock() {
            q.push_back(r);
        }
    }
}

/// Extensions from the standard places: `~/.psi/extensions`, the trusted
/// project's `.psi/extensions`, and paths given with `--extension`.
pub fn sources(
    home: Option<&Path>,
    project: Option<&Path>,
    extra: &[std::path::PathBuf],
) -> (Vec<Source>, Vec<String>) {
    let mut found = Vec::new();
    if let Some(h) = home {
        found.extend(load::discover(&h.join(".psi/extensions")));
    }
    if let Some(p) = project {
        found.extend(load::discover(&p.join(".psi/extensions")));
    }
    for e in extra {
        match load::source(e) {
            Some(r) => found.push(r),
            None => found.push(Err(format!("{}: not an extension (a .lua file, a directory with init.lua, or a directory with tool.json)", e.display()))),
        }
    }
    let mut ok = Vec::new();
    let mut errors = Vec::new();
    for r in found {
        match r {
            Ok(s) => ok.push(s),
            Err(e) => errors.push(e),
        }
    }
    (ok, errors)
}
