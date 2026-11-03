//! One extension = one Lua state on its own thread (D-13), answering
//! messages from psi. Async host functions (`ui.confirm`) run on the
//! thread's own single-threaded runtime and look blocking from Lua.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use mlua::serde::SerializeOptions;
use mlua::{Function, Lua, LuaSerdeExt, Table, Value};
use serde_json::Value as Json;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use crate::host::{Backend, Host, Registrations, Request, Shared, context};
use crate::limits::{self, Limits};

/// How long loading and setup may take, in total.
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Notify,
    Chain,
    Gate,
}

/// The outcome of one event in one extension.
#[derive(Debug, Clone, Default)]
pub struct EventOut {
    /// The payload after this extension's handlers (chain and gate merge
    /// their results into it).
    pub payload: Json,
    /// What each handler returned (non-nil only).
    pub results: Vec<Json>,
}

pub enum Msg {
    Event {
        name: String,
        kind: Kind,
        payload: Json,
        reply: oneshot::Sender<Result<EventOut, String>>,
    },
    Tool {
        name: String,
        args: Json,
        reply: oneshot::Sender<(String, bool)>,
    },
    Command {
        name: String,
        args: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Shortcut {
        key: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Render {
        tool: String,
        call: Json,
        width: usize,
        reply: std::sync::mpsc::Sender<Option<Vec<String>>>,
    },
    ComponentRender {
        id: u64,
        width: usize,
        reply: std::sync::mpsc::Sender<Vec<String>>,
    },
    ComponentInput {
        id: u64,
        key: String,
        reply: std::sync::mpsc::Sender<Option<Json>>,
    },
    Bus {
        name: String,
        data: Json,
    },
    Shutdown,
}

/// A running extension, as psi holds it.
pub struct Actor {
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: PathBuf,
    pub regs: Registrations,
    pub tx: UnboundedSender<Msg>,
}

struct ChannelBackend(UnboundedSender<(String, Request)>);

impl Backend for ChannelBackend {
    fn request(&self, ext: &str, req: Request) {
        let _ = self.0.send((ext.to_owned(), req));
    }
}

/// JSON nulls become nil, not a sentinel.
fn opts() -> SerializeOptions {
    SerializeOptions::new()
        .serialize_none_to_null(false)
        .serialize_unit_to_null(false)
}

pub fn to_lua(lua: &Lua, v: &Json) -> mlua::Result<Value> {
    lua.to_value_with(v, opts())
}

/// Loads `path` and runs its setup on a new thread.
pub fn spawn(
    path: &Path,
    shared: Arc<RwLock<Shared>>,
    requests: UnboundedSender<(String, Request)>,
    limits: Limits,
) -> Result<Actor, String> {
    let (tx, rx) = unbounded_channel();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let file = path.to_owned();
    let thread_name = format!(
        "ext {}",
        path.file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    );
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ =
                        ready_tx.send(Err(format!("cannot start the extension's runtime: {e}")));
                    return;
                }
            };
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, run(file, shared, requests, limits, rx, ready_tx));
        })
        .map_err(|e| format!("cannot start a thread: {e}"))?;
    let (name, version, description, regs) =
        ready_rx.recv_timeout(LOAD_TIMEOUT).map_err(|_| {
            format!(
                "{}: loading took more than {}s",
                path.display(),
                LOAD_TIMEOUT.as_secs()
            )
        })??;
    Ok(Actor {
        name,
        version,
        description,
        path: path.to_owned(),
        regs,
        tx,
    })
}

type Ready = Result<(String, String, String, Registrations), String>;

pub struct State<B: Backend> {
    pub lua: Lua,
    pub host: Rc<Host<B>>,
    pub psi: Table,
    pub limits: Limits,
}

impl<B: Backend> State<B> {
    /// Calls Lua from psi, under the busy limit.
    pub async fn call<R: mlua::FromLuaMulti>(
        &self,
        f: Function,
        args: impl mlua::IntoLuaMulti,
    ) -> mlua::Result<R> {
        let t = limits::thread(&self.lua, f, &self.host.busy, self.limits.busy)?;
        self.host.busy.start();
        let r = t.into_async::<R>(args)?.await;
        self.host.busy.stop();
        r
    }

    fn ctx(&self) -> mlua::Result<Table> {
        let shared = self
            .host
            .shared
            .read()
            .map(|s| s.clone())
            .unwrap_or_default();
        context(&self.lua, &self.psi, &shared)
    }

    fn report(&self, what: &str, e: &mlua::Error) {
        self.host.backend.request(
            &self.host.ext,
            Request::Notify {
                message: format!("{what} failed: {}", first_line(e)),
                level: "error".into(),
            },
        );
    }
}

/// The error's message without mlua's traceback.
pub fn first_line(e: &mlua::Error) -> String {
    let s = match e {
        mlua::Error::CallbackError { cause, .. } => return first_line(cause),
        mlua::Error::RuntimeError(m) => m.clone(),
        other => other.to_string(),
    };
    s.lines().next().unwrap_or("").to_owned()
}

/// Loads an extension file and runs its setup.
pub async fn load<B: Backend>(
    file: &Path,
    shared: Arc<RwLock<Shared>>,
    backend: B,
    limits: Limits,
) -> Result<(State<B>, (String, String, String)), String> {
    let shown = file.display().to_string();
    let lua = Lua::new();
    let busy = limits::install(&lua, &limits).map_err(|e| format!("{shown}: {e}"))?;
    psi_ext_api::batteries::remove_shell(&lua).map_err(|e| format!("{shown}: {e}"))?;
    // `require` finds the extension's own modules first.
    if let Some(dir) = file.parent() {
        let pkg: Table = lua.globals().get("package").map_err(|e| e.to_string())?;
        let old: String = pkg.get("path").unwrap_or_default();
        let d = dir.display();
        let _ = pkg.set("path", format!("{d}/?.lua;{d}/?/init.lua;{old}"));
    }
    let code = std::fs::read_to_string(file).map_err(|e| format!("{shown}: {e}"))?;
    let chunk = lua
        .load(code)
        .set_name(format!("@{shown}"))
        .into_function()
        .map_err(|e| first_line(&e))?;
    let t = limits::thread(&lua, chunk, &busy, limits.busy).map_err(|e| e.to_string())?;
    busy.start();
    let manifest = t.into_async::<Value>(()).map_err(|e| e.to_string())?.await;
    busy.stop();
    let Value::Table(m) = manifest.map_err(|e| first_line(&e))? else {
        return Err(format!(
            "{shown}: an extension returns a table: {{ name = ..., version = ..., description = ..., setup = function(psi) ... end }}"
        ));
    };
    let get = |k: &str| -> Result<String, String> {
        m.get::<String>(k).map_err(|_| {
            format!("{shown}: the table it returns (the manifest) needs a string '{k}'")
        })
    };
    let (name, version, description) = (get("name")?, get("version")?, get("description")?);
    let setup: Function = m
        .get("setup")
        .map_err(|_| format!("{shown}: the table it returns (the manifest) needs 'setup', a function: return {{ name = ..., version = ..., description = ..., setup = setup }}"))?;
    let host = Host::new(&name, backend, shared, busy);
    let psi = psi_ext_api::register(&lua, host.clone()).map_err(|e| e.to_string())?;
    let _ = lua.globals().set("psi", psi.clone());
    let state = State {
        lua,
        host,
        psi,
        limits,
    };
    state
        .call::<()>(setup, state.psi.clone())
        .await
        .map_err(|e| format!("{name}: setup failed: {}", first_line(&e)))?;
    Ok((state, (name, version, description)))
}

async fn run(
    file: PathBuf,
    shared: Arc<RwLock<Shared>>,
    requests: UnboundedSender<(String, Request)>,
    limits: Limits,
    mut rx: UnboundedReceiver<Msg>,
    ready: std::sync::mpsc::Sender<Ready>,
) {
    let (state, (name, version, description)) =
        match load(&file, shared, ChannelBackend(requests), limits).await {
            Ok(s) => s,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
    let regs = state.host.reg.borrow().summary();
    let _ = ready.send(Ok((name, version, description, regs)));
    let state = Rc::new(state);
    while let Some(msg) = rx.recv().await {
        if matches!(msg, Msg::Shutdown) {
            break;
        }
        let s = state.clone();
        tokio::task::spawn_local(async move { handle(&s, msg).await });
    }
}

/// Merges a handler's result into the payload: fields replace, except
/// `system`, which accumulates.
fn merge(payload: &mut Json, result: &Json) {
    let (Json::Object(p), Json::Object(r)) = (payload, result) else {
        return;
    };
    for (k, v) in r {
        match (k.as_str(), p.get(k)) {
            ("system", Some(Json::String(old))) if !old.is_empty() => {
                let joined = format!("{old}\n\n{}", v.as_str().unwrap_or(""));
                p.insert(k.clone(), Json::String(joined));
            }
            _ => {
                p.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Answers one message from psi.
pub async fn handle<B: Backend>(s: &State<B>, msg: Msg) {
    match msg {
        Msg::Event {
            name,
            kind,
            mut payload,
            reply,
        } => {
            let handlers = s
                .host
                .reg
                .borrow()
                .handlers
                .get(&name)
                .cloned()
                .unwrap_or_default();
            let mut results = Vec::new();
            for h in handlers {
                let args = (to_lua(&s.lua, &payload), s.ctx());
                let r = match args {
                    (Ok(ev), Ok(ctx)) => s.call::<Value>(h, (ev, ctx)).await,
                    (Err(e), _) | (_, Err(e)) => Err(e),
                };
                match r {
                    Ok(Value::Nil) => {}
                    Ok(v) => {
                        let j: Json = s.lua.from_value(v).unwrap_or(Json::Null);
                        if kind != Kind::Notify {
                            merge(&mut payload, &j);
                        }
                        let block = kind == Kind::Gate && j["block"] == Json::Bool(true);
                        results.push(j);
                        if block {
                            break;
                        }
                    }
                    Err(e) if kind == Kind::Notify => s.report(&format!("the {name} handler"), &e),
                    Err(e) => {
                        let _ = reply.send(Err(first_line(&e)));
                        return;
                    }
                }
            }
            let _ = reply.send(Ok(EventOut { payload, results }));
        }
        Msg::Tool { name, args, reply } => {
            let f = s.host.reg.borrow().tools.get(&name).map(|(_, f)| f.clone());
            let out = match f {
                None => (format!("There is no tool named '{name}'."), true),
                Some(f) => match (to_lua(&s.lua, &args), s.ctx()) {
                    (Ok(a), Ok(ctx)) => match s.call::<Value>(f, (a, ctx)).await {
                        Ok(Value::String(t)) => (t.to_string_lossy(), false),
                        Ok(Value::Table(t)) => (
                            t.get::<Option<String>>("text")
                                .ok()
                                .flatten()
                                .unwrap_or_default(),
                            t.get::<Option<bool>>("is_error")
                                .ok()
                                .flatten()
                                .unwrap_or(false),
                        ),
                        Ok(Value::Nil) => ("(no output)".into(), false),
                        Ok(other) => (format!("{other:?}"), false),
                        Err(e) => (format!("The tool failed: {}", first_line(&e)), true),
                    },
                    (Err(e), _) | (_, Err(e)) => {
                        (format!("The tool failed: {}", first_line(&e)), true)
                    }
                },
            };
            let _ = reply.send(out);
        }
        Msg::Command { name, args, reply } => {
            let f = s
                .host
                .reg
                .borrow()
                .commands
                .get(&name)
                .map(|(_, f)| f.clone());
            let r = match (f, s.ctx()) {
                (None, _) => Err(format!("no command /{name}")),
                (Some(f), Ok(ctx)) => s
                    .call::<()>(f, (args, ctx))
                    .await
                    .map_err(|e| first_line(&e)),
                (_, Err(e)) => Err(e.to_string()),
            };
            let _ = reply.send(r);
        }
        Msg::Shortcut { key, reply } => {
            let f = s
                .host
                .reg
                .borrow()
                .shortcuts
                .get(&key)
                .map(|(_, f)| f.clone());
            let r = match (f, s.ctx()) {
                (None, _) => Err(format!("no shortcut {key}")),
                (Some(f), Ok(ctx)) => s.call::<()>(f, ctx).await.map_err(|e| first_line(&e)),
                (_, Err(e)) => Err(e.to_string()),
            };
            let _ = reply.send(r);
        }
        Msg::Render {
            tool,
            call,
            width,
            reply,
        } => {
            let f = s.host.reg.borrow().renderers.get(&tool).cloned();
            let lines = match (f, to_lua(&s.lua, &call)) {
                (Some(f), Ok(c)) => s.call::<Vec<String>>(f, (c, width)).await.ok(),
                _ => None,
            };
            let _ = reply.send(lines);
        }
        Msg::ComponentRender { id, width, reply } => {
            let t = s.host.reg.borrow().components.get(&id).cloned();
            let lines = match t.and_then(|t| t.get::<Function>("render").ok()) {
                Some(f) => s
                    .call::<Vec<String>>(f, width)
                    .await
                    .unwrap_or_else(|e| vec![format!("render failed: {}", first_line(&e))]),
                None => Vec::new(),
            };
            let _ = reply.send(lines);
        }
        Msg::ComponentInput { id, key, reply } => {
            let t = s.host.reg.borrow().components.get(&id).cloned();
            let out = match t.and_then(|t| t.get::<Function>("handle_input").ok()) {
                Some(f) => match s.call::<Value>(f, key).await {
                    Ok(Value::Nil) => None,
                    Ok(v) => Some(s.lua.from_value(v).unwrap_or(Json::Null)),
                    Err(e) => {
                        s.report("a component", &e);
                        Some(Json::Null)
                    }
                },
                None => Some(Json::Null),
            };
            let _ = reply.send(out);
        }
        Msg::Bus { name, data } => {
            let handlers = s
                .host
                .reg
                .borrow()
                .bus
                .get(&name)
                .cloned()
                .unwrap_or_default();
            for h in handlers {
                if let Ok(d) = to_lua(&s.lua, &data)
                    && let Err(e) = s.call::<()>(h, d).await
                {
                    s.report(&format!("the events.on('{name}') handler"), &e);
                }
            }
        }
        Msg::Shutdown => {}
    }
}
