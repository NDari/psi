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
    fn register_provider(&self, _: &Lua, name: String, _: Value) -> mlua::Result<()> {
        self.record(format!("register_provider {name}"));
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
    async fn exec(&self, _: &Lua, argv: Vec<String>, _: Option<Value>) -> mlua::Result<Value> {
        self.record(format!("exec {argv:?}"));
        Ok(Value::Nil)
    }
    async fn http_request(&self, _: &Lua, req: Value) -> mlua::Result<Value> {
        self.record(format!("http_request {}", kind(&req)));
        Ok(Value::Nil)
    }
    async fn http_get(&self, _: &Lua, url: String) -> mlua::Result<Value> {
        self.record(format!("http_get {url}"));
        Ok(Value::Nil)
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

/// Runs Lua that asserts, with the builtin batteries from `register`.
fn run(lua: &Lua, code: &str) {
    if let Err(e) = lua.load(code).exec() {
        panic!("{e}");
    }
}

#[test]
fn json_round_trips_and_sorts_keys() {
    let (lua, _) = setup(Recorder::default());
    run(
        &lua,
        r#"
        local j = psi.json
        assert(j.encode({ b = 1, a = { 1, 2, "x" }, c = true }) == '{"a":[1,2,"x"],"b":1,"c":true}')
        assert(j.encode({}) == "{}")
        assert(j.encode(j.array({})) == "[]")
        assert(j.encode({ n = j.null() }) == '{"n":null}')
        local v = j.decode('{"list":[1,null,3],"s":"é","f":1.5}')
        assert(#v.list == 3 and v.list[2] == j.null())
        assert(v.s == "é" and v.f == 1.5)
        assert(j.encode(v.list) == "[1,null,3]")
        assert(j.encode(j.decode("[]")) == "[]")
        assert(j.encode({ a = 1 }, { pretty = true }) == '{\n  "a": 1\n}')
        "#,
    );
    assert!(lua_error(&lua, "psi.json.decode('{')").starts_with("psi.json.decode: EOF"));
    assert!(
        lua_error(&lua, "psi.json.encode({ f = print })").starts_with("psi.json.encode: "),
        "{}",
        lua_error(&lua, "psi.json.encode({ f = print })")
    );
}

#[test]
fn regexes_find_replace_and_split() {
    let (lua, _) = setup(Recorder::default());
    run(
        &lua,
        r#"
        local re = psi.re
        assert(re.is_match("^\\d+$", "123") and not re.is_match("^\\d+$", "12a"))
        local m = re.find("(?P<k>\\w+)=(\\d+)?", "x: key=")
        assert(m.text == "key=" and m.start == 4 and m.finish == 7)
        assert(m.groups[1] == "key" and m.groups[2] == "" and m.named.k == "key")
        assert(re.find("z", "abc") == nil)
        local all = re.find_all("\\d", "a1b22")
        assert(#all == 3 and all[3].start == 5)
        assert(re.replace("(\\w)(\\d)", "a1 b2", "$2$1") == "1a 2b")
        assert(re.replace("o", "foo", "0", 1) == "f0o")
        local parts = re.split(",\\s*", "a, b,c")
        assert(#parts == 3 and parts[2] == "b")
        local dir = "/home/a.b (1)/c+d"
        assert(re.is_match("^" .. re.escape(dir), dir .. "/x"))
        assert(not re.is_match("^" .. re.escape(dir), "/home/aXb (1)/c+d/x"))
        "#,
    );
    assert!(
        lua_error(&lua, "psi.re.is_match('(', 'x')").starts_with("psi.re.is_match: bad pattern")
    );
}

#[test]
fn paths_are_lexical() {
    let (lua, _) = setup(Recorder::default());
    run(
        &lua,
        r#"
        local p = psi.path
        assert(p.join("a/b", "c.txt") == "a/b/c.txt")
        assert(p.join("a", "/abs") == "/abs")
        assert(p.normalize("a/./b/../c") == "a/c")
        assert(p.relative("/w/src/x.rs", "/w/docs") == "../src/x.rs")
        assert(p.relative("/w", "/w") == ".")
        assert(p.basename("a/b/c.txt") == "c.txt" and p.dirname("a/b/c.txt") == "a/b")
        assert(p.dirname("c.txt") == "." and p.dirname("/") == "/")
        assert(p.ext("a/c.tar.gz") == "gz" and p.ext("Makefile") == "")
        assert(p.absolute("/x/../y") == "/y")
        "#,
    );
}

#[test]
fn files_read_write_list_and_glob() {
    let (lua, _) = setup(Recorder::default());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".gitignore"), "skip/\n").unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    lua.globals()
        .set("D", dir.path().display().to_string())
        .unwrap();
    run(
        &lua,
        r#"
        local fs, p = psi.fs, psi.path
        fs.write(p.join(D, "src/a.rs"), "fn a() {}\0bytes")
        fs.write(p.join(D, "src/deep/b.rs"), "b")
        fs.write(p.join(D, "skip/c.rs"), "c")
        assert(fs.read(p.join(D, "src/a.rs")) == "fn a() {}\0bytes")
        assert(fs.exists(p.join(D, "src")) and not fs.exists(p.join(D, "nope")))
        local st = fs.stat(p.join(D, "src/a.rs"))
        assert(st.kind == "file" and st.size == 15 and st.modified > 0)
        assert(fs.stat(p.join(D, "src")).kind == "dir" and fs.stat(p.join(D, "nope")) == nil)
        local names = fs.list(p.join(D, "src"))
        assert(#names == 2 and names[1] == "a.rs" and names[2] == "deep")
        local top = fs.glob("src/*.rs", { cwd = D })
        assert(#top == 1 and top[1] == "src/a.rs", table.concat(top, " "))
        local all = fs.glob("**/*.rs", { cwd = D })
        assert(#all == 2 and all[2] == "src/deep/b.rs", table.concat(all, " "))
        local every = fs.glob("**/*.rs", { cwd = D, gitignore = false })
        assert(#every == 3, table.concat(every, " "))
        "#,
    );
    let err = lua_error(&lua, "psi.fs.read(D .. '/nope')");
    assert!(
        err.starts_with("psi.fs.read: ") && err.contains("nope"),
        "{err}"
    );
}

#[test]
fn formats_hashes_and_time() {
    let (lua, _) = setup(Recorder::default());
    run(
        &lua,
        r#"
        local t = psi.toml.decode('name = "psi"\nwhen = 2026-11-02T10:00:00Z\n[deps]\nmlua = { version = "0.12" }\n')
        assert(t.name == "psi" and t.deps.mlua.version == "0.12", psi.json.encode(t))
        assert(t.when == "2026-11-02T10:00:00Z", psi.json.encode(t))
        assert(psi.toml.encode({ b = 1, a = "x" }) == 'a = "x"\nb = 1\n')
        assert(psi.base64.encode("hi\0") == "aGkA" and psi.base64.decode("aGkA") == "hi\0")
        assert(psi.hash.sha256("abc") == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        local now = psi.time.now()
        assert(now > 1.7e9)
        assert(psi.time.parse("2026-11-02T10:00:00Z") == 1793613600)
        assert(psi.time.parse("not a time") == nil)
        assert(psi.time.parse("2026-11-02 10:00 +0000", "%Y-%m-%d %H:%M %z") == 1793613600)
        assert(psi.time.format(0, "%Y") == "1970" or psi.time.format(0, "%Y") == "1969")
        assert(#psi.time.format() >= 20)
        "#,
    );
    assert!(lua_error(&lua, "psi.base64.decode('!!')").starts_with("psi.base64.decode: "));
    assert!(lua_error(&lua, "psi.time.format(0, '%Q')").starts_with("psi.time.format: bad format"));
    assert!(lua_error(&lua, "psi.toml.encode({1, 2})").starts_with("psi.toml.encode: "));
}
