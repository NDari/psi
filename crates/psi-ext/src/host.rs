//! The Lua side of one extension: what it registered, and the `ExtHost`
//! implementation behind the generated `psi` table (D-14). Everything here
//! lives on the extension's own thread (D-13).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use mlua::{Function, Lua, LuaSerdeExt, Table, Value};
use psi_ext_api::ExtHost;
use serde::Serialize;
use serde_json::Value as Json;
use tokio::sync::oneshot;

use crate::limits::Busy;

/// What an extension asks of psi. UI requests go to the interactive UI
/// (or are answered for a headless run); the rest go to the agent.
#[derive(Debug)]
pub enum Request {
    Notify {
        message: String,
        level: String,
    },
    Confirm {
        title: String,
        message: String,
        reply: oneshot::Sender<bool>,
    },
    Select {
        title: String,
        options: Vec<String>,
        reply: oneshot::Sender<Option<String>>,
    },
    Input {
        title: String,
        placeholder: Option<String>,
        reply: oneshot::Sender<Option<String>>,
    },
    Status {
        key: String,
        text: Option<String>,
    },
    Widget {
        slot: String,
        lines: Option<Vec<String>>,
    },
    /// Show component `id` until a key answers it.
    Custom {
        id: u64,
        reply: oneshot::Sender<Option<Json>>,
    },
    SendUserMessage {
        text: String,
        follow_up: bool,
    },
    SendMessage {
        custom_type: String,
        text: String,
    },
    AppendEntry {
        custom_type: String,
        data: Json,
    },
    SetActiveTools(Vec<String>),
    ExecuteTool {
        name: String,
        args: Json,
        reply: oneshot::Sender<(String, bool)>,
    },
    /// events.emit: to every extension's events.on handlers.
    Bus {
        name: String,
        data: Json,
    },
    Log(String),
    /// Answered once every request sent before it was handled.
    Flush(oneshot::Sender<()>),
    /// psi.exec: run through the agent's policy and sandbox.
    Exec {
        call: ExecCall,
        reply: oneshot::Sender<Result<ExecOutput, String>>,
    },
    /// psi.http.request: honors the `network` setting.
    Http {
        call: HttpCall,
        reply: oneshot::Sender<Result<HttpOutput, String>>,
    },
}

/// A `psi.exec` call.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecCall {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    /// Per stream.
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecOutput {
    /// `None` when it was stopped.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub killed: bool,
    pub truncated: bool,
}

/// A `psi.http.request` call.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpCall {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HttpOutput {
    pub status: u16,
    /// Lowercase names; a repeated header joined with ", ".
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub truncated: bool,
}

const EXEC_TIMEOUT_MS: i64 = 60_000;
const EXEC_MAX_BYTES: i64 = 1 << 20;
const HTTP_TIMEOUT_MS: i64 = 30_000;
const HTTP_MAX_BYTES: i64 = 10 << 20;

/// A positive `opts` number, else `default`.
fn positive(t: &Table, key: &str, default: i64) -> mlua::Result<u64> {
    let v: Option<i64> = t.get(key)?;
    Ok(v.filter(|n| *n > 0).unwrap_or(default) as u64)
}

/// A `{string: string}` field as pairs, sorted.
fn pairs(t: &Table, key: &str) -> mlua::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    if let Some(m) = t.get::<Option<Table>>(key)? {
        for p in m.pairs::<String, String>() {
            out.push(p?);
        }
    }
    out.sort();
    Ok(out)
}

/// The `opts` of psi.exec.
pub fn exec_call(argv: Vec<String>, opts: Option<Value>) -> mlua::Result<ExecCall> {
    if argv.is_empty() {
        return Err(err(
            "psi.exec: argv is empty; give the program and its arguments, like {\"git\", \"status\"}",
        ));
    }
    let t = match opts {
        Some(Value::Table(t)) => Some(t),
        _ => None,
    };
    let Some(t) = t else {
        return Ok(ExecCall {
            argv,
            cwd: None,
            env: Vec::new(),
            stdin: None,
            timeout: Duration::from_millis(EXEC_TIMEOUT_MS as u64),
            max_bytes: EXEC_MAX_BYTES as usize,
        });
    };
    Ok(ExecCall {
        argv,
        cwd: t.get("cwd")?,
        env: pairs(&t, "env")?,
        stdin: t
            .get::<Option<mlua::LuaString>>("stdin")?
            .map(|s| s.as_bytes().to_vec()),
        timeout: Duration::from_millis(positive(&t, "timeout_ms", EXEC_TIMEOUT_MS)?),
        max_bytes: positive(&t, "max_bytes", EXEC_MAX_BYTES)? as usize,
    })
}

/// The table psi.exec returns.
pub fn exec_result(lua: &Lua, o: &ExecOutput) -> mlua::Result<Value> {
    let t = lua.create_table()?;
    t.set("code", o.code)?;
    t.set("ok", o.code == Some(0))?;
    t.set("stdout", lua.create_string(&o.stdout)?)?;
    t.set("stderr", lua.create_string(&o.stderr)?)?;
    t.set("killed", o.killed)?;
    t.set("truncated", o.truncated)?;
    Ok(Value::Table(t))
}

/// The request table of psi.http.request.
pub fn http_call(lua: &Lua, req: Value) -> mlua::Result<HttpCall> {
    let what = "psi.http.request";
    let Value::Table(t) = req else {
        return Err(err(format!("{what}: expected a table with a url")));
    };
    let url: String = field(&t, what, "url")?;
    let mut headers = pairs(&t, "headers")?;
    let mut body = t
        .get::<Option<mlua::LuaString>>("body")?
        .map(|s| s.as_bytes().to_vec());
    let json: Value = t.get("json")?;
    if !json.is_nil() {
        let j = psi_ext_api::batteries::to_json(lua, what, json)?;
        body = Some(j.to_string().into_bytes());
        if !headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        {
            headers.push(("content-type".into(), "application/json".into()));
        }
    }
    let method = match t.get::<Option<String>>("method")? {
        Some(m) => m.to_ascii_uppercase(),
        None if body.is_some() => "POST".into(),
        None => "GET".into(),
    };
    Ok(HttpCall {
        method,
        url,
        headers,
        query: pairs(&t, "query")?,
        body,
        timeout: Duration::from_millis(positive(&t, "timeout_ms", HTTP_TIMEOUT_MS)?),
        max_bytes: positive(&t, "max_bytes", HTTP_MAX_BYTES)? as usize,
    })
}

/// A GET for psi.http.get.
pub fn http_get_call(url: String) -> HttpCall {
    HttpCall {
        method: "GET".into(),
        url,
        headers: Vec::new(),
        query: Vec::new(),
        body: None,
        timeout: Duration::from_millis(HTTP_TIMEOUT_MS as u64),
        max_bytes: HTTP_MAX_BYTES as usize,
    }
}

/// The table psi.http.request returns; `json()` decodes the body.
pub fn http_result(lua: &Lua, o: HttpOutput) -> mlua::Result<Value> {
    let t = lua.create_table()?;
    t.set("status", o.status)?;
    t.set("ok", (200..300).contains(&o.status))?;
    let h = lua.create_table()?;
    for (k, v) in &o.headers {
        h.set(k.as_str(), v.as_str())?;
    }
    t.set("headers", h)?;
    let body = lua.create_string(&o.body)?;
    t.set("body", &body)?;
    t.set("truncated", o.truncated)?;
    t.set(
        "json",
        lua.create_function(move |lua, ()| psi_ext_api::batteries::json_decode(lua, body.clone()))?,
    )?;
    Ok(Value::Table(t))
}

/// Where an extension's requests go.
pub trait Backend: 'static {
    fn request(&self, ext: &str, req: Request);
}

/// State psi keeps current for extensions to read without waiting.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Shared {
    pub cwd: String,
    /// interactive, print, json or test.
    pub mode: String,
    pub has_ui: bool,
    pub session_file: Option<String>,
    pub active_tools: Vec<String>,
    /// Custom entries on the branch: (custom_type, data).
    #[serde(skip)]
    pub entries: Vec<(String, Json)>,
    /// `--ext-flag name=value`.
    #[serde(skip)]
    pub flags: BTreeMap<String, String>,
}

/// One registered tool, as psi sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolInfo {
    pub ext: String,
    pub name: String,
    pub description: String,
    /// (name, type, description, required), in declared order.
    pub params: Vec<(String, String, String, bool)>,
    pub read_only: bool,
    pub always_ask: bool,
}

/// A provider from `psi.register_provider`: an OpenAI-compatible endpoint.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderInfo {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: Vec<(String, String)>,
    pub models: Vec<ProviderModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderModel {
    pub id: String,
    pub context_window: Option<u64>,
    pub max_output: Option<u64>,
    pub enable_thinking: Option<bool>,
}

/// What an extension registered, sent to psi after setup.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Registrations {
    pub tools: Vec<ToolInfo>,
    /// (name, description)
    pub commands: Vec<(String, String)>,
    pub shortcuts: Vec<(String, String)>,
    pub flags: Vec<(String, String)>,
    pub events: Vec<String>,
    pub renderers: Vec<String>,
    pub providers: Vec<ProviderInfo>,
}

/// The Lua values behind the registrations.
#[derive(Default)]
pub struct Registry {
    pub handlers: BTreeMap<String, Vec<Function>>,
    pub tools: BTreeMap<String, (ToolInfo, Function)>,
    pub commands: BTreeMap<String, (String, Function)>,
    pub shortcuts: BTreeMap<String, (String, Function)>,
    pub flags: BTreeMap<String, (String, Value)>,
    pub renderers: BTreeMap<String, Function>,
    pub providers: BTreeMap<String, ProviderInfo>,
    pub bus: BTreeMap<String, Vec<Function>>,
    pub components: BTreeMap<u64, Table>,
    pub next_component: u64,
}

impl Registry {
    pub fn summary(&self) -> Registrations {
        Registrations {
            tools: self.tools.values().map(|(t, _)| t.clone()).collect(),
            commands: self
                .commands
                .iter()
                .map(|(n, (d, _))| (n.clone(), d.clone()))
                .collect(),
            shortcuts: self
                .shortcuts
                .iter()
                .map(|(n, (d, _))| (n.clone(), d.clone()))
                .collect(),
            flags: self
                .flags
                .iter()
                .map(|(n, (d, _))| (n.clone(), d.clone()))
                .collect(),
            events: self.handlers.keys().cloned().collect(),
            renderers: self.renderers.keys().cloned().collect(),
            providers: self.providers.values().cloned().collect(),
        }
    }
}

pub struct Host<B: Backend> {
    pub ext: String,
    pub reg: RefCell<Registry>,
    pub backend: B,
    pub shared: Arc<RwLock<Shared>>,
    pub busy: Busy,
}

fn err(msg: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(msg.into())
}

fn field<T: mlua::FromLua>(t: &Table, what: &str, key: &str) -> mlua::Result<T> {
    t.get::<T>(key).map_err(|_| {
        err(format!(
            "{what}: field '{key}' is missing or has the wrong type"
        ))
    })
}

impl<B: Backend> Host<B> {
    pub fn new(ext: &str, backend: B, shared: Arc<RwLock<Shared>>, busy: Busy) -> Rc<Self> {
        Rc::new(Self {
            ext: ext.into(),
            reg: RefCell::new(Registry::default()),
            backend,
            shared,
            busy,
        })
    }

    fn send(&self, r: Request) {
        self.backend.request(&self.ext, r);
    }

    /// Awaits psi without counting toward the busy limit.
    async fn wait<T>(&self, rx: oneshot::Receiver<T>, default: T) -> T {
        self.busy.stop();
        let v = rx.await.unwrap_or(default);
        self.busy.start();
        v
    }

    fn shared(&self) -> Shared {
        self.shared.read().map(|s| s.clone()).unwrap_or_default()
    }
}

/// A tool name the model can use.
fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && n.chars().next().is_some_and(|c| c.is_ascii_lowercase())
}

const COLORS: &[(&str, u8)] = &[
    ("black", 30),
    ("red", 31),
    ("green", 32),
    ("yellow", 33),
    ("blue", 34),
    ("magenta", 35),
    ("cyan", 36),
    ("white", 37),
    ("bright_black", 90),
    ("bright_red", 91),
    ("bright_green", 92),
    ("bright_yellow", 93),
    ("bright_blue", 94),
    ("bright_magenta", 95),
    ("bright_cyan", 96),
    ("bright_white", 97),
];

impl<B: Backend> ExtHost for Host<B> {
    fn on(&self, _: &Lua, event: String, handler: Function) -> mlua::Result<()> {
        self.reg
            .borrow_mut()
            .handlers
            .entry(event)
            .or_default()
            .push(handler);
        Ok(())
    }

    fn register_tool(&self, lua: &Lua, spec: Value) -> mlua::Result<()> {
        let Value::Table(t) = spec else {
            return Err(err("psi.register_tool: expected a table"));
        };
        let what = "psi.register_tool";
        let name: String = field(&t, what, "name")?;
        if !valid_name(&name) {
            return Err(err(format!(
                "{what}: name '{name}' must be lowercase letters, digits and _"
            )));
        }
        let description: String = field(&t, what, "description")?;
        let execute: Function = field(&t, what, "execute")?;
        let params: Table = field(&t, what, "params")?;
        let required: Option<Vec<String>> = t.get("required")?;
        let mut ps = Vec::new();
        {
            // Declared order isn't kept by Lua tables; sort for a stable schema.
            let mut names: Vec<(String, Table)> = params
                .pairs::<String, Table>()
                .collect::<mlua::Result<_>>()?;
            names.sort_by(|a, b| a.0.cmp(&b.0));
            for (pname, p) in names {
                let pw = format!("{what}: param '{pname}'");
                let ty: String = field(&p, &pw, "type")?;
                if !["string", "integer", "number", "boolean", "array", "object"]
                    .contains(&ty.as_str())
                {
                    return Err(err(format!(
                        "{what}: param '{pname}' has type '{ty}'; use string, integer, number, boolean, array or object"
                    )));
                }
                let desc: String = field(&p, &pw, "description")?;
                let req = required.as_ref().is_none_or(|r| r.contains(&pname));
                ps.push((pname, ty, desc, req));
            }
        }
        let ann: Option<Table> = t.get("annotations")?;
        let read_only = ann
            .as_ref()
            .and_then(|a| a.get::<Option<bool>>("read_only").ok().flatten())
            .unwrap_or(false);
        let always_ask: bool = t.get::<Option<bool>>("always_ask")?.unwrap_or(false);
        let _ = lua;
        let info = ToolInfo {
            ext: self.ext.clone(),
            name: name.clone(),
            description,
            params: ps,
            read_only,
            always_ask,
        };
        self.reg.borrow_mut().tools.insert(name, (info, execute));
        Ok(())
    }

    fn register_command(&self, _: &Lua, name: String, spec: Value) -> mlua::Result<()> {
        let Value::Table(t) = spec else {
            return Err(err("psi.register_command: expected a table"));
        };
        let what = "psi.register_command";
        let desc: String = field(&t, what, "description")?;
        let handler: Function = field(&t, what, "handler")?;
        let name = name.trim_start_matches('/').to_owned();
        self.reg.borrow_mut().commands.insert(name, (desc, handler));
        Ok(())
    }

    fn register_shortcut(&self, _: &Lua, key: String, spec: Value) -> mlua::Result<()> {
        let Value::Table(t) = spec else {
            return Err(err("psi.register_shortcut: expected a table"));
        };
        let what = "psi.register_shortcut";
        let desc: String = field(&t, what, "description")?;
        let handler: Function = field(&t, what, "handler")?;
        self.reg
            .borrow_mut()
            .shortcuts
            .insert(key.to_ascii_lowercase(), (desc, handler));
        Ok(())
    }

    fn register_provider(&self, _: &Lua, name: String, spec: Value) -> mlua::Result<()> {
        let what = "psi.register_provider";
        let Value::Table(t) = spec else {
            return Err(err(format!("{what}: expected a table")));
        };
        if name.trim().is_empty() || name.contains('/') {
            return Err(err(format!(
                "{what}: the name must be non-empty, without '/'"
            )));
        }
        let base_url: String = field(&t, what, "base_url")?;
        let api_key: Option<String> = t.get("api_key")?;
        let mut headers = Vec::new();
        if let Some(h) = t.get::<Option<Table>>("headers")? {
            for pair in h.pairs::<String, String>() {
                headers.push(pair?);
            }
        }
        headers.sort();
        let list: Table = field(&t, what, "models")?;
        let mut models = Vec::new();
        for m in list.sequence_values::<Table>() {
            let m = m?;
            let mw = format!("{what}: a model");
            models.push(ProviderModel {
                id: field(&m, &mw, "id")?,
                context_window: m.get("context_window")?,
                max_output: m.get("max_output")?,
                enable_thinking: m.get("enable_thinking")?,
            });
        }
        if models.is_empty() {
            return Err(err(format!("{what}: 'models' lists no models")));
        }
        let info = ProviderInfo {
            name: name.clone(),
            base_url,
            api_key,
            headers,
            models,
        };
        self.reg.borrow_mut().providers.insert(name, info);
        Ok(())
    }

    fn register_flag(&self, _: &Lua, name: String, spec: Value) -> mlua::Result<()> {
        let Value::Table(t) = spec else {
            return Err(err("psi.register_flag: expected a table"));
        };
        let desc: String = field(&t, "psi.register_flag", "description")?;
        let default: Value = t.get("default")?;
        self.reg.borrow_mut().flags.insert(name, (desc, default));
        Ok(())
    }

    fn get_flag(&self, lua: &Lua, name: String) -> mlua::Result<Value> {
        if let Some(v) = self.shared().flags.get(&name) {
            return Ok(Value::String(lua.create_string(v)?));
        }
        Ok(self
            .reg
            .borrow()
            .flags
            .get(&name)
            .map(|(_, d)| d.clone())
            .unwrap_or(Value::Nil))
    }

    fn register_tool_renderer(&self, _: &Lua, tool: String, render: Function) -> mlua::Result<()> {
        self.reg.borrow_mut().renderers.insert(tool, render);
        Ok(())
    }

    fn send_user_message(&self, _: &Lua, text: String, opts: Option<Value>) -> mlua::Result<()> {
        let follow_up = match opts {
            Some(Value::Table(t)) => {
                t.get::<Option<String>>("deliver")?.as_deref() == Some("follow_up")
            }
            _ => false,
        };
        self.send(Request::SendUserMessage { text, follow_up });
        Ok(())
    }

    fn send_message(&self, _: &Lua, custom_type: String, text: String) -> mlua::Result<()> {
        self.send(Request::SendMessage { custom_type, text });
        Ok(())
    }

    fn append_entry(&self, lua: &Lua, custom_type: String, data: Value) -> mlua::Result<()> {
        let data: Json = lua.from_value(data)?;
        // Readable at once, before psi writes it.
        if let Ok(mut s) = self.shared.write() {
            s.entries.push((custom_type.clone(), data.clone()));
        }
        self.send(Request::AppendEntry { custom_type, data });
        Ok(())
    }

    fn entries(&self, lua: &Lua, custom_type: String) -> mlua::Result<Value> {
        let list: Vec<Json> = self
            .shared()
            .entries
            .into_iter()
            .filter(|(t, _)| *t == custom_type)
            .map(|(_, d)| d)
            .collect();
        lua.to_value(&list)
    }

    fn set_active_tools(&self, _: &Lua, names: Vec<String>) -> mlua::Result<()> {
        if let Ok(mut s) = self.shared.write() {
            s.active_tools = names.clone();
        }
        self.send(Request::SetActiveTools(names));
        Ok(())
    }

    fn get_active_tools(&self, _: &Lua) -> mlua::Result<Vec<String>> {
        Ok(self.shared().active_tools)
    }

    async fn execute_tool(&self, lua: &Lua, name: String, args: Value) -> mlua::Result<Value> {
        let args: Json = lua.from_value(args)?;
        let (tx, rx) = oneshot::channel();
        self.send(Request::ExecuteTool {
            name,
            args,
            reply: tx,
        });
        let (text, is_error) = self.wait(rx, ("psi is shutting down".into(), true)).await;
        let t = lua.create_table()?;
        t.set("text", text)?;
        t.set("is_error", is_error)?;
        Ok(Value::Table(t))
    }

    fn events_emit(&self, lua: &Lua, name: String, data: Value) -> mlua::Result<()> {
        let data: Json = lua.from_value(data)?;
        self.send(Request::Bus { name, data });
        Ok(())
    }

    fn events_on(&self, _: &Lua, name: String, handler: Function) -> mlua::Result<()> {
        self.reg
            .borrow_mut()
            .bus
            .entry(name)
            .or_default()
            .push(handler);
        Ok(())
    }

    fn ui_notify(&self, _: &Lua, message: String, level: Option<String>) -> mlua::Result<()> {
        self.send(Request::Notify {
            message,
            level: level.unwrap_or_else(|| "info".into()),
        });
        Ok(())
    }

    async fn ui_confirm(&self, _: &Lua, title: String, message: String) -> mlua::Result<bool> {
        let (tx, rx) = oneshot::channel();
        self.send(Request::Confirm {
            title,
            message,
            reply: tx,
        });
        Ok(self.wait(rx, false).await)
    }

    async fn ui_select(
        &self,
        _: &Lua,
        title: String,
        options: Vec<String>,
    ) -> mlua::Result<Option<String>> {
        let (tx, rx) = oneshot::channel();
        self.send(Request::Select {
            title,
            options,
            reply: tx,
        });
        Ok(self.wait(rx, None).await)
    }

    async fn ui_input(
        &self,
        _: &Lua,
        title: String,
        placeholder: Option<String>,
    ) -> mlua::Result<Option<String>> {
        let (tx, rx) = oneshot::channel();
        self.send(Request::Input {
            title,
            placeholder,
            reply: tx,
        });
        Ok(self.wait(rx, None).await)
    }

    fn ui_status(&self, _: &Lua, key: String, text: Option<String>) -> mlua::Result<()> {
        self.send(Request::Status { key, text });
        Ok(())
    }

    fn ui_widget(&self, _: &Lua, slot: String, lines: Option<Vec<String>>) -> mlua::Result<()> {
        self.send(Request::Widget { slot, lines });
        Ok(())
    }

    async fn ui_custom(&self, lua: &Lua, component: Value) -> mlua::Result<Value> {
        let Value::Table(t) = component else {
            return Err(err("psi.ui.custom: expected a component table"));
        };
        let _: Function = field(&t, "psi.ui.custom", "render")?;
        let _: Function = field(&t, "psi.ui.custom", "handle_input")?;
        let id = {
            let mut r = self.reg.borrow_mut();
            r.next_component += 1;
            let id = r.next_component;
            r.components.insert(id, t.clone());
            id
        };
        let (tx, rx) = oneshot::channel();
        self.send(Request::Custom { id, reply: tx });
        let answer = self.wait(rx, None).await;
        let t = self.reg.borrow_mut().components.remove(&id);
        if let Some(t) = t
            && let Ok(Some(dispose)) = t.get::<Option<Function>>("dispose")
        {
            let _ = dispose.call::<()>(());
        }
        match answer {
            Some(v) => lua.to_value(&v),
            None => Ok(Value::Nil),
        }
    }

    fn style_color(&self, _: &Lua, color: String, text: String) -> mlua::Result<String> {
        let Some((_, code)) = COLORS.iter().find(|(n, _)| *n == color) else {
            let names: Vec<&str> = COLORS.iter().map(|(n, _)| *n).collect();
            return Err(err(format!(
                "psi.style.color: no color '{color}'; use one of {}",
                names.join(", ")
            )));
        };
        Ok(format!("\x1b[{code}m{text}\x1b[39m"))
    }

    fn style_bold(&self, _: &Lua, text: String) -> mlua::Result<String> {
        Ok(format!("\x1b[1m{text}\x1b[22m"))
    }

    fn style_dim(&self, _: &Lua, text: String) -> mlua::Result<String> {
        Ok(format!("\x1b[2m{text}\x1b[22m"))
    }

    fn log(&self, _: &Lua, message: String) -> mlua::Result<()> {
        self.send(Request::Log(message));
        Ok(())
    }

    async fn exec(&self, lua: &Lua, argv: Vec<String>, opts: Option<Value>) -> mlua::Result<Value> {
        let call = exec_call(argv, opts)?;
        let (tx, rx) = oneshot::channel();
        self.send(Request::Exec { call, reply: tx });
        match self.wait(rx, Err("psi is shutting down".into())).await {
            Ok(o) => exec_result(lua, &o),
            Err(m) => Err(err(format!("psi.exec: {m}"))),
        }
    }

    async fn http_request(&self, lua: &Lua, req: Value) -> mlua::Result<Value> {
        let call = http_call(lua, req)?;
        self.http(lua, call).await
    }

    async fn http_get(&self, lua: &Lua, url: String) -> mlua::Result<Value> {
        self.http(lua, http_get_call(url)).await
    }
}

impl<B: Backend> Host<B> {
    async fn http(&self, lua: &Lua, call: HttpCall) -> mlua::Result<Value> {
        let (tx, rx) = oneshot::channel();
        self.send(Request::Http { call, reply: tx });
        match self.wait(rx, Err("psi is shutting down".into())).await {
            Ok(o) => http_result(lua, o),
            Err(m) => Err(err(format!("psi.http: {m}"))),
        }
    }
}

/// The `ctx` handlers get: the shared facts plus psi.ui and execute_tool.
pub fn context(lua: &Lua, psi: &Table, shared: &Shared) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("cwd", shared.cwd.as_str())?;
    t.set("mode", shared.mode.as_str())?;
    t.set("has_ui", shared.has_ui)?;
    t.set("session_file", shared.session_file.as_deref())?;
    t.set("ui", psi.get::<Table>("ui")?)?;
    t.set("execute_tool", psi.get::<Function>("execute_tool")?)?;
    Ok(t)
}
