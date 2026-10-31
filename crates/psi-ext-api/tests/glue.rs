#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! The generated glue, driven from Lua against a recording host.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Value};
use psi_ext_api::{ExtHost, FUNCTIONS, register};

/// Records every call as `method arg arg ...`; refuses `refuse` in `check`.
#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<String>>,
    refuse: Option<&'static str>,
}

impl Recorder {
    fn record(&self, call: String) {
        self.calls.borrow_mut().push(call);
    }
}

fn kind(v: &Value) -> &'static str {
    v.type_name()
}

impl ExtHost for Recorder {
    fn check(&self, function: &'static str) -> mlua::Result<()> {
        if self.refuse == Some(function) {
            return Err(mlua::Error::runtime(format!("{function} is not allowed")));
        }
        Ok(())
    }
    fn on(&self, _: &Lua, event: String, _: Function) -> mlua::Result<()> {
        self.record(format!("on {event}"));
        Ok(())
    }
    fn register_tool(&self, _: &Lua, spec: Value) -> mlua::Result<()> {
        self.record(format!("register_tool {}", kind(&spec)));
        Ok(())
    }
    fn register_command(&self, _: &Lua, name: String, _: Value) -> mlua::Result<()> {
        self.record(format!("register_command {name}"));
        Ok(())
    }
    fn register_shortcut(&self, _: &Lua, key: String, _: Value) -> mlua::Result<()> {
        self.record(format!("register_shortcut {key}"));
        Ok(())
    }
    fn register_flag(&self, _: &Lua, name: String, _: Value) -> mlua::Result<()> {
        self.record(format!("register_flag {name}"));
        Ok(())
    }
    fn get_flag(&self, _: &Lua, name: String) -> mlua::Result<Value> {
        self.record(format!("get_flag {name}"));
        Ok(Value::Integer(7))
    }
    fn register_tool_renderer(&self, _: &Lua, tool: String, _: Function) -> mlua::Result<()> {
        self.record(format!("register_tool_renderer {tool}"));
        Ok(())
    }
    fn send_user_message(&self, _: &Lua, text: String, opts: Option<Value>) -> mlua::Result<()> {
        self.record(format!("send_user_message {text} {}", opts.is_some()));
        Ok(())
    }
    fn send_message(&self, _: &Lua, custom_type: String, text: String) -> mlua::Result<()> {
        self.record(format!("send_message {custom_type} {text}"));
        Ok(())
    }
    fn append_entry(&self, _: &Lua, custom_type: String, data: Value) -> mlua::Result<()> {
        self.record(format!("append_entry {custom_type} {}", kind(&data)));
        Ok(())
    }
    fn entries(&self, lua: &Lua, custom_type: String) -> mlua::Result<Value> {
        self.record(format!("entries {custom_type}"));
        Ok(Value::Table(lua.create_table()?))
    }
    fn set_active_tools(&self, _: &Lua, names: Vec<String>) -> mlua::Result<()> {
        self.record(format!("set_active_tools {names:?}"));
        Ok(())
    }
    fn get_active_tools(&self, _: &Lua) -> mlua::Result<Vec<String>> {
        self.record("get_active_tools".into());
        Ok(vec!["read".into(), "bash".into()])
    }
    async fn execute_tool(&self, lua: &Lua, name: String, _: Value) -> mlua::Result<Value> {
        self.record(format!("execute_tool {name}"));
        let result = lua.create_table()?;
        result.set("text", "ok")?;
        Ok(Value::Table(result))
    }
    fn events_emit(&self, _: &Lua, name: String, data: Value) -> mlua::Result<()> {
        self.record(format!("events_emit {name} {}", kind(&data)));
        Ok(())
    }
    fn events_on(&self, _: &Lua, name: String, _: Function) -> mlua::Result<()> {
        self.record(format!("events_on {name}"));
        Ok(())
    }
    fn ui_notify(&self, _: &Lua, message: String, level: Option<String>) -> mlua::Result<()> {
        self.record(format!("ui_notify {message} {level:?}"));
        Ok(())
    }
    async fn ui_confirm(&self, _: &Lua, title: String, message: String) -> mlua::Result<bool> {
        // Really suspend, so the call goes through the async path.
        tokio::task::yield_now().await;
        self.record(format!("ui_confirm {title} {message}"));
        Ok(true)
    }
    async fn ui_select(
        &self,
        _: &Lua,
        title: String,
        options: Vec<String>,
    ) -> mlua::Result<Option<String>> {
        self.record(format!("ui_select {title} {options:?}"));
        Ok(options.into_iter().next())
    }
    async fn ui_input(
        &self,
        _: &Lua,
        title: String,
        placeholder: Option<String>,
    ) -> mlua::Result<Option<String>> {
        self.record(format!("ui_input {title} {placeholder:?}"));
        Ok(None)
    }
    fn ui_status(&self, _: &Lua, key: String, text: Option<String>) -> mlua::Result<()> {
        self.record(format!("ui_status {key} {text:?}"));
        Ok(())
    }
    fn ui_widget(&self, _: &Lua, slot: String, lines: Option<Vec<String>>) -> mlua::Result<()> {
        self.record(format!("ui_widget {slot} {lines:?}"));
        Ok(())
    }
    async fn ui_custom(&self, _: &Lua, component: Value) -> mlua::Result<Value> {
        self.record(format!("ui_custom {}", kind(&component)));
        Ok(Value::Nil)
    }
    fn style_color(&self, _: &Lua, color: String, text: String) -> mlua::Result<String> {
        Ok(format!("<{color}>{text}"))
    }
    fn style_bold(&self, _: &Lua, text: String) -> mlua::Result<String> {
        Ok(format!("*{text}*"))
    }
    fn style_dim(&self, _: &Lua, text: String) -> mlua::Result<String> {
        Ok(format!("~{text}~"))
    }
    fn log(&self, _: &Lua, message: String) -> mlua::Result<()> {
        self.record(format!("log {message}"));
        Ok(())
    }
}

fn setup(host: Recorder) -> (Lua, Rc<Recorder>) {
    let lua = Lua::new();
    let host = Rc::new(host);
    let psi = register(&lua, Rc::clone(&host)).unwrap();
    lua.globals().set("psi", psi).unwrap();
    (lua, host)
}

/// The first line of the error Lua sees when `code` fails under pcall.
fn lua_error(lua: &Lua, code: &str) -> String {
    let err: String = lua
        .load(format!(
            "local ok, err = pcall(function() {code} end)\n\
             assert(not ok, 'expected an error')\n\
             return tostring(err)"
        ))
        .eval()
        .unwrap();
    err.lines().next().unwrap_or_default().to_owned()
}

#[test]
fn a_call_reaches_the_host() {
    let (lua, host) = setup(Recorder::default());
    lua.load(r#"psi.log("x")"#).exec().unwrap();
    assert_eq!(*host.calls.borrow(), ["log x"]);
}

#[test]
fn returns_come_back_to_lua() {
    let (lua, _) = setup(Recorder::default());
    let (bold, tools, flag, entries): (String, Vec<String>, i64, String) = lua
        .load(r#"return psi.style.bold("b"), psi.get_active_tools(), psi.get_flag("f"), type(psi.entries("t"))"#)
        .eval()
        .unwrap();
    assert_eq!(bold, "*b*");
    assert_eq!(tools, ["read", "bash"]);
    assert_eq!(flag, 7);
    assert_eq!(entries, "table");
}

#[test]
fn wrong_type() {
    let (lua, host) = setup(Recorder::default());
    assert_eq!(
        lua_error(&lua, "psi.log(1)"),
        "psi.log: argument 1 'message' expected string, got number"
    );
    assert_eq!(
        lua_error(&lua, "psi.register_tool('t')"),
        "psi.register_tool: argument 1 'spec' expected ToolSpec, got string"
    );
    assert_eq!(
        lua_error(&lua, "psi.events.on('x', {})"),
        "psi.events.on: argument 2 'handler' expected function, got table"
    );
    assert_eq!(
        lua_error(&lua, "psi.set_active_tools({'read', 2})"),
        "psi.set_active_tools: argument 1 'names' expected string[], got number at [2]"
    );
    assert!(host.calls.borrow().is_empty());
}

#[test]
fn the_rust_side_sees_the_same_message() {
    let (lua, _) = setup(Recorder::default());
    let err = lua.load("psi.log(true)").exec().unwrap_err().to_string();
    assert!(
        err.starts_with("psi.log: argument 1 'message' expected string, got boolean\n"),
        "{err}"
    );
}

#[test]
fn missing_required_is_nil() {
    let (lua, _) = setup(Recorder::default());
    assert_eq!(
        lua_error(&lua, "psi.send_message('kind')"),
        "psi.send_message: argument 2 'text' expected string, got nil"
    );
    assert_eq!(
        lua_error(&lua, "psi.log()"),
        "psi.log: argument 1 'message' expected string, got nil"
    );
}

#[test]
fn literal_union() {
    let (lua, _) = setup(Recorder::default());
    assert_eq!(
        lua_error(&lua, "psi.ui.notify('m', 'warn')"),
        r#"psi.ui.notify: argument 2 'level' expected "info"|"warning"|"error", got "warn""#
    );
    assert_eq!(
        lua_error(&lua, "psi.ui.notify('m', 3)"),
        r#"psi.ui.notify: argument 2 'level' expected "info"|"warning"|"error", got number"#
    );
    // The event name is the union of every event.
    let err = lua_error(&lua, "psi.on('tool_calls', function() end)");
    assert!(
        err.starts_with(r#"psi.on: argument 1 'event' expected "session_start"|"#),
        "{err}"
    );
    assert!(
        err.ends_with(r#"|"guard_event", got "tool_calls""#),
        "{err}"
    );
}

#[test]
fn too_many_arguments() {
    let (lua, _) = setup(Recorder::default());
    assert_eq!(
        lua_error(&lua, "psi.log('x', 'y')"),
        "psi.log: expected 1 argument, got 2"
    );
    assert_eq!(
        lua_error(&lua, "psi.get_active_tools(1)"),
        "psi.get_active_tools: expected no arguments, got 1"
    );
    assert_eq!(
        lua_error(&lua, "psi.ui.notify('m', 'info', 3)"),
        "psi.ui.notify: expected at most 2 arguments, got 3"
    );
}

#[test]
fn optional_params_may_be_omitted() {
    let (lua, host) = setup(Recorder::default());
    lua.load(
        r#"
        psi.ui.notify("a")
        psi.ui.notify("b", nil)
        psi.ui.notify("c", "error")
        psi.ui.widget("w")
        psi.ui.widget("w", {"l1", "l2"})
        psi.send_user_message("hi")
        psi.send_user_message("hi", { deliver = "follow_up" })
        psi.on("tool_call", function(ev, ctx) end)
        psi.append_entry("k", nil)
        "#,
    )
    .exec()
    .unwrap();
    assert_eq!(
        *host.calls.borrow(),
        [
            "ui_notify a None",
            "ui_notify b None",
            r#"ui_notify c Some("error")"#,
            "ui_widget w None",
            r#"ui_widget w Some(["l1", "l2"])"#,
            "send_user_message hi false",
            "send_user_message hi true",
            "on tool_call",
            "append_entry k nil",
        ]
    );
}

#[test]
fn every_function_is_in_its_table() {
    let (lua, _) = setup(Recorder::default());
    let psi: mlua::Table = lua.globals().get("psi").unwrap();
    for t in ["ui", "events", "style"] {
        assert!(psi.get::<Value>(t).unwrap().is_table(), "psi.{t}");
    }
    for f in FUNCTIONS {
        let found: Value = lua.load(format!("return psi.{}", f.name)).eval().unwrap();
        assert!(
            found.is_function(),
            "psi.{} is {}",
            f.name,
            found.type_name()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn async_functions_yield_and_return() {
    let (lua, host) = setup(Recorder::default());
    let (yes, pick, text, tool): (bool, Option<String>, Option<String>, String) = lua
        .load(
            r#"
            local yes = psi.ui.confirm("Title", "Sure?")
            local pick = psi.ui.select("Pick", {"a", "b"})
            local text = psi.ui.input("Name")
            local tool = psi.execute_tool("read", { path = "x" })
            return yes, pick, text, tool.text
            "#,
        )
        .call_async(())
        .await
        .unwrap();
    assert!(yes);
    assert_eq!(pick.as_deref(), Some("a"));
    assert_eq!(text, None);
    assert_eq!(tool, "ok");
    assert_eq!(host.calls.borrow()[0], "ui_confirm Title Sure?");
}

#[tokio::test(flavor = "current_thread")]
async fn async_functions_check_arguments() {
    let (lua, _) = setup(Recorder::default());
    let err = lua
        .load(r#"psi.ui.confirm("only a title")"#)
        .exec_async()
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.starts_with("psi.ui.confirm: argument 2 'message' expected string, got nil\n"),
        "{err}"
    );
}

#[test]
fn check_can_refuse_a_call() {
    let (lua, host) = setup(Recorder {
        refuse: Some("ui.status"),
        ..Recorder::default()
    });
    assert_eq!(
        lua_error(&lua, "psi.ui.status('k', 'v')"),
        "runtime error: ui.status is not allowed"
    );
    // Refused before the arguments are looked at.
    assert_eq!(
        lua_error(&lua, "psi.ui.status()"),
        "runtime error: ui.status is not allowed"
    );
    lua.load("psi.log('still fine')").exec().unwrap();
    assert_eq!(*host.calls.borrow(), ["log still fine"]);
}
