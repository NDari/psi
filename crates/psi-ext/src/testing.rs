//! `psi ext test` (psi-plan.md §9.6): runs an extension's `*_test.lua`
//! files against a fake host, in the extension's own Lua state, with
//! scripted UI answers, mock tools, a temporary working directory and a
//! virtual clock. It shows everything the extension printed, logged,
//! notified or threw, not just the failing assertion.
//!
//! In a test file, `test` holds the helpers:
//!
//! ```lua
//! test.case("blocks force pushes", function()
//!   local r = test.emit("tool_call", { id = "c1", name = "bash", args = { command = "git push -f" } })
//!   test.eq(r.payload.block, true)
//! end)
//! ```

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use mlua::{Function, LuaSerdeExt, Table, Value};
use serde_json::{Value as Json, json};
use tokio::sync::oneshot;

use crate::actor::{self, Kind, Msg, State, first_line, to_lua};
use crate::host::{Backend, Request, Shared};
use crate::limits::Limits;
use crate::{kind_of, load};

#[derive(Default)]
struct Fake {
    /// Every request, as a table-like record for `test.requests()`.
    requests: Vec<Json>,
    /// Lines the extension printed, logged or notified.
    output: Vec<String>,
    /// Scripted answers by kind: confirm, select, input, custom, exec, http.
    answers: BTreeMap<String, VecDeque<Json>>,
    /// Mock tools for `psi.execute_tool`: name to result.
    tools: BTreeMap<String, Json>,
}

#[derive(Clone, Default)]
struct FakeBackend(Rc<RefCell<Fake>>);

impl FakeBackend {
    fn answer(&self, kind: &str) -> Option<Json> {
        self.0
            .borrow_mut()
            .answers
            .get_mut(kind)
            .and_then(VecDeque::pop_front)
    }
}

impl Backend for FakeBackend {
    fn request(&self, _ext: &str, req: Request) {
        let record = |f: &FakeBackend, v: Json| f.0.borrow_mut().requests.push(v);
        match req {
            Request::Notify { message, level } => {
                self.0
                    .borrow_mut()
                    .output
                    .push(format!("notify ({level}): {message}"));
                record(
                    self,
                    json!({"kind": "notify", "message": message, "level": level}),
                );
            }
            Request::Log(m) => {
                self.0.borrow_mut().output.push(format!("log: {m}"));
                record(self, json!({"kind": "log", "message": m}));
            }
            Request::Flush(reply) => {
                let _ = reply.send(());
            }
            Request::Confirm {
                title,
                message,
                reply,
            } => {
                record(
                    self,
                    json!({"kind": "confirm", "title": title, "message": message}),
                );
                let _ = reply.send(
                    self.answer("confirm")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                );
            }
            Request::Select {
                title,
                options,
                reply,
            } => {
                record(
                    self,
                    json!({"kind": "select", "title": title, "options": options}),
                );
                let _ = reply.send(
                    self.answer("select")
                        .and_then(|v| v.as_str().map(str::to_owned)),
                );
            }
            Request::Input {
                title,
                placeholder,
                reply,
            } => {
                record(
                    self,
                    json!({"kind": "input", "title": title, "placeholder": placeholder}),
                );
                let _ = reply.send(
                    self.answer("input")
                        .and_then(|v| v.as_str().map(str::to_owned)),
                );
            }
            Request::Custom { id, reply } => {
                record(self, json!({"kind": "custom", "id": id}));
                let _ = reply.send(self.answer("custom"));
            }
            Request::Status { key, text } => {
                record(self, json!({"kind": "status", "key": key, "text": text}))
            }
            Request::Widget { slot, lines } => record(
                self,
                json!({"kind": "widget", "slot": slot, "lines": lines}),
            ),
            Request::SendUserMessage { text, follow_up } => {
                record(
                    self,
                    json!({"kind": "send_user_message", "text": text, "follow_up": follow_up}),
                );
            }
            Request::SendMessage { custom_type, text } => {
                record(
                    self,
                    json!({"kind": "send_message", "custom_type": custom_type, "text": text}),
                );
            }
            Request::AppendEntry { custom_type, data } => {
                record(
                    self,
                    json!({"kind": "append_entry", "custom_type": custom_type, "data": data}),
                );
            }
            Request::SetActiveTools(names) => {
                record(self, json!({"kind": "set_active_tools", "names": names}))
            }
            Request::Bus { name, data } => {
                record(self, json!({"kind": "emit", "name": name, "data": data}))
            }
            Request::Exec { call, reply } => {
                record(
                    self,
                    json!({"kind": "exec", "argv": call.argv, "cwd": call.cwd}),
                );
                let out = match self.answer("exec") {
                    Some(v) => Ok(crate::host::ExecOutput {
                        code: Some(v["code"].as_i64().unwrap_or(0) as i32),
                        stdout: v["stdout"].as_str().unwrap_or("").as_bytes().to_vec(),
                        stderr: v["stderr"].as_str().unwrap_or("").as_bytes().to_vec(),
                        ..Default::default()
                    }),
                    None => Err(format!(
                        "test: no answer for {} (use test.ui.answer(\"exec\", {{ code = 0, stdout = \"...\" }}))",
                        call.argv.join(" ")
                    )),
                };
                let _ = reply.send(out);
            }
            Request::Http { call, reply } => {
                record(
                    self,
                    json!({"kind": "http", "method": call.method, "url": call.url}),
                );
                let out = match self.answer("http") {
                    Some(v) => Ok(crate::host::HttpOutput {
                        status: v["status"].as_u64().unwrap_or(200) as u16,
                        body: v["body"].as_str().unwrap_or("").as_bytes().to_vec(),
                        ..Default::default()
                    }),
                    None => Err(format!(
                        "test: no answer for {} {} (use test.ui.answer(\"http\", {{ status = 200, body = \"...\" }}))",
                        call.method, call.url
                    )),
                };
                let _ = reply.send(out);
            }
            Request::ExecuteTool { name, args, reply } => {
                record(
                    self,
                    json!({"kind": "execute_tool", "name": name, "args": args}),
                );
                let r = self.0.borrow().tools.get(&name).cloned();
                let out = match r {
                    Some(Json::String(t)) => (t, false),
                    Some(v) => (
                        v["text"].as_str().unwrap_or("").to_owned(),
                        v["is_error"].as_bool().unwrap_or(false),
                    ),
                    None => (
                        format!("test: no mock for tool '{name}' (use test.mock_tool)"),
                        true,
                    ),
                };
                let _ = reply.send(out);
            }
        }
    }
}

/// One test case's outcome.
#[derive(Debug, Clone)]
pub struct CaseResult {
    pub file: PathBuf,
    pub name: String,
    pub error: Option<String>,
    /// What the extension printed, logged or notified during the case.
    pub output: Vec<String>,
}

#[derive(Debug, Default)]
pub struct TestReport {
    pub cases: Vec<CaseResult>,
    /// Problems before any case ran: loading, setup, a test file's syntax.
    pub errors: Vec<String>,
}

impl TestReport {
    pub fn passed(&self) -> bool {
        self.errors.is_empty() && self.cases.iter().all(|c| c.error.is_none())
    }

    pub fn render(&self, verbose: bool) -> String {
        let mut s = String::new();
        for e in &self.errors {
            s.push_str(&format!("error: {e}\n"));
        }
        for c in &self.cases {
            let file = c
                .file
                .file_name()
                .map(|f| f.to_string_lossy())
                .unwrap_or_default();
            match &c.error {
                None => s.push_str(&format!("ok    {file}: {}\n", c.name)),
                Some(e) => s.push_str(&format!("FAIL  {file}: {}\n      {e}\n", c.name)),
            }
            if c.error.is_some() || verbose {
                for o in &c.output {
                    s.push_str(&format!("      | {o}\n"));
                }
            }
        }
        let failed = self.cases.iter().filter(|c| c.error.is_some()).count();
        s.push_str(&format!(
            "{} passed, {failed} failed{}\n",
            self.cases.len() - failed,
            if self.errors.is_empty() {
                String::new()
            } else {
                format!(", {} errors", self.errors.len())
            }
        ));
        s
    }
}

/// The extension and its test files at `path` (a directory, or a `.lua`
/// file with `<name>_test.lua` beside it).
fn layout(path: &Path) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let (main, dir) = if path.is_dir() {
        (path.join("init.lua"), path.to_owned())
    } else {
        (
            path.to_owned(),
            path.parent().map(Path::to_owned).unwrap_or_default(),
        )
    };
    if !main.is_file() {
        return Err(format!(
            "{}: no extension here (a .lua file or a directory with init.lua)",
            path.display()
        ));
    }
    let stem = main
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut tests: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            n.ends_with("_test.lua") && (path.is_dir() || n == format!("{stem}_test.lua"))
        })
        .collect();
    tests.sort();
    Ok((main, tests))
}

/// Runs every test of the extension at `path`.
pub fn run(path: &Path) -> TestReport {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            return TestReport {
                errors: vec![e.to_string()],
                ..TestReport::default()
            };
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, run_async(path))
}

async fn run_async(path: &Path) -> TestReport {
    let mut report = TestReport::default();
    let (main, tests) = match layout(path) {
        Ok(l) => l,
        Err(e) => {
            report.errors.push(e);
            return report;
        }
    };
    if tests.is_empty() {
        report
            .errors
            .push(format!("{}: no *_test.lua files", path.display()));
        return report;
    }
    for t in tests {
        run_file(&main, &t, &mut report).await;
    }
    report
}

/// Each test file gets a fresh extension, so files don't share state.
async fn run_file(main: &Path, test_file: &Path, report: &mut TestReport) {
    let tmp = match tempfile::tempdir() {
        Ok(t) => t,
        Err(e) => {
            report.errors.push(e.to_string());
            return;
        }
    };
    let shared = Arc::new(RwLock::new(Shared {
        cwd: tmp.path().display().to_string(),
        mode: "test".into(),
        has_ui: true,
        ..Shared::default()
    }));
    let fake = FakeBackend::default();
    let limits = Limits {
        busy: std::time::Duration::from_secs(2),
        ..Limits::default()
    };
    let (state, _) = match actor::load(main, shared, fake.clone(), limits).await {
        Ok(s) => s,
        Err(e) => {
            report.errors.push(e);
            return;
        }
    };
    let state = Rc::new(state);
    let cases: Rc<RefCell<Vec<(String, Function)>>> = Rc::default();
    if let Err(e) = install(&state, &fake, &cases, tmp.path()) {
        report
            .errors
            .push(format!("cannot set up the test helpers: {e}"));
        return;
    }
    let code = match std::fs::read_to_string(test_file) {
        Ok(c) => c,
        Err(e) => {
            report.errors.push(format!("{}: {e}", test_file.display()));
            return;
        }
    };
    let chunk = state
        .lua
        .load(code)
        .set_name(format!("@{}", test_file.display()));
    if let Err(e) = chunk.exec_async().await {
        report
            .errors
            .push(format!("{}: {}", test_file.display(), first_line(&e)));
        return;
    }
    let list = cases.borrow().clone();
    for (name, f) in list {
        // Each case starts clean: no requests, answers or output.
        {
            let mut fk = fake.0.borrow_mut();
            fk.output.clear();
            fk.requests.clear();
            fk.answers.clear();
        }
        if let Ok(mut sh) = state.host.shared.write() {
            sh.entries.clear();
            sh.flags.clear();
        }
        let r = state.call::<()>(f, ()).await;
        let output = std::mem::take(&mut fake.0.borrow_mut().output);
        report.cases.push(CaseResult {
            file: test_file.to_owned(),
            name,
            error: r.err().map(|e| case_error(&e)),
            output,
        });
    }
}

/// The failure with its location, without mlua's Rust-side noise.
fn case_error(e: &mlua::Error) -> String {
    let (first, traceback) = match e {
        mlua::Error::CallbackError { traceback, cause } => (first_line(cause), traceback.clone()),
        other => (first_line(other), other.to_string()),
    };
    let first = first.trim_start_matches("runtime error: ").to_owned();
    if first.contains(".lua:") {
        return first;
    }
    // Otherwise the first frame in a .lua file is where it failed.
    let at = traceback
        .lines()
        .map(str::trim)
        .find(|l| l.contains(".lua:") && !l.contains("[C]"))
        .map(|l| {
            format!(
                " (at {})",
                l.split(": in ").next().unwrap_or(l).trim_end_matches(':')
            )
        });
    format!("{first}{}", at.unwrap_or_default())
}

fn install(
    s: &Rc<State<FakeBackend>>,
    fake: &FakeBackend,
    cases: &Rc<RefCell<Vec<(String, Function)>>>,
    tmp: &Path,
) -> mlua::Result<()> {
    let lua = &s.lua;
    let t = lua.create_table()?;
    t.set("tmp", tmp.display().to_string())?;
    let c = cases.clone();
    t.set(
        "case",
        lua.create_function(move |_, (name, f): (String, Function)| {
            c.borrow_mut().push((name, f));
            Ok(())
        })?,
    )?;
    // Events, tools, commands and components, through the same code psi uses.
    let st = s.clone();
    t.set(
        "emit",
        lua.create_async_function(move |lua, (event, payload): (String, Value)| {
            let st = st.clone();
            async move {
                let payload: Json = lua.from_value(payload)?;
                let kind = kind_of(&event);
                let (tx, rx) = oneshot::channel();
                actor::handle(
                    &st,
                    Msg::Event {
                        name: event,
                        kind,
                        payload,
                        reply: tx,
                    },
                )
                .await;
                match rx.await.map_err(mlua::Error::external)? {
                    Ok(o) => {
                        let r = lua.create_table()?;
                        r.set("payload", to_lua(&lua, &o.payload)?)?;
                        r.set("results", to_lua(&lua, &Json::Array(o.results))?)?;
                        if kind == Kind::Gate {
                            let blocked = r
                                .get::<Table>("payload")?
                                .get::<Option<bool>>("block")?
                                .unwrap_or(false);
                            r.set("blocked", blocked)?;
                        }
                        Ok(Value::Table(r))
                    }
                    Err(e) => Err(mlua::Error::runtime(format!(
                        "the {kind:?} handler failed: {e}"
                    ))),
                }
            }
        })?,
    )?;
    let st = s.clone();
    t.set(
        "tool",
        lua.create_async_function(move |lua, (name, args): (String, Value)| {
            let st = st.clone();
            async move {
                let args: Json = lua.from_value(args)?;
                let (tx, rx) = oneshot::channel();
                actor::handle(
                    &st,
                    Msg::Tool {
                        name,
                        args,
                        reply: tx,
                    },
                )
                .await;
                let (text, is_error) = rx.await.map_err(mlua::Error::external)?;
                let r = lua.create_table()?;
                r.set("text", text)?;
                r.set("is_error", is_error)?;
                Ok(r)
            }
        })?,
    )?;
    let st = s.clone();
    t.set(
        "command",
        lua.create_async_function(move |_, (name, args): (String, Option<String>)| {
            let st = st.clone();
            async move {
                let (tx, rx) = oneshot::channel();
                let name = name.trim_start_matches('/').to_owned();
                actor::handle(
                    &st,
                    Msg::Command {
                        name,
                        args: args.unwrap_or_default(),
                        reply: tx,
                    },
                )
                .await;
                rx.await
                    .map_err(mlua::Error::external)?
                    .map_err(mlua::Error::runtime)
            }
        })?,
    )?;
    let st = s.clone();
    t.set(
        "shortcut",
        lua.create_async_function(move |_, key: String| {
            let st = st.clone();
            async move {
                let (tx, rx) = oneshot::channel();
                actor::handle(&st, Msg::Shortcut { key, reply: tx }).await;
                rx.await
                    .map_err(mlua::Error::external)?
                    .map_err(mlua::Error::runtime)
            }
        })?,
    )?;
    let st = s.clone();
    t.set(
        "render",
        lua.create_async_function(
            move |lua, (tool, call, width): (String, Value, Option<usize>)| {
                let st = st.clone();
                async move {
                    let call: Json = lua.from_value(call)?;
                    let (tx, rx) = std::sync::mpsc::channel();
                    actor::handle(
                        &st,
                        Msg::Render {
                            tool,
                            call,
                            width: width.unwrap_or(80),
                            reply: tx,
                        },
                    )
                    .await;
                    Ok(rx.recv().ok().flatten())
                }
            },
        )?,
    )?;
    // Scripted answers and mocks.
    let f = fake.clone();
    let ui = lua.create_table()?;
    ui.set(
        "answer",
        lua.create_function(move |lua, (kind, v): (String, Value)| {
            let v: Json = lua.from_value(v).map_err(|e| {
                mlua::Error::runtime(format!(
                    "test.ui.answer({kind:?}, ...): the answer must be a plain value ({e}); {}",
                    answer_shape(&kind)
                ))
            })?;
            f.0.borrow_mut()
                .answers
                .entry(kind)
                .or_default()
                .push_back(v);
            Ok(())
        })?,
    )?;
    t.set("ui", ui)?;
    let f = fake.clone();
    t.set(
        "mock_tool",
        lua.create_function(move |lua, (name, result): (String, Value)| {
            let r: Json = lua.from_value(result).map_err(|e| {
                mlua::Error::runtime(format!(
                    "test.mock_tool({name:?}, ...): the result must be a string or a plain table ({e})"
                ))
            })?;
            f.0.borrow_mut().tools.insert(name, r);
            Ok(())
        })?,
    )?;
    let f = fake.clone();
    t.set(
        "requests",
        lua.create_function(move |lua, kind: Option<String>| {
            let all = f.0.borrow().requests.clone();
            let list: Vec<Json> = all
                .into_iter()
                .filter(|r| kind.as_deref().is_none_or(|k| r["kind"] == k))
                .collect();
            to_lua(lua, &Json::Array(list))
        })?,
    )?;
    let f = fake.clone();
    t.set(
        "clear",
        lua.create_function(move |_, ()| {
            f.0.borrow_mut().requests.clear();
            Ok(())
        })?,
    )?;
    // test.providers(): what psi.register_provider registered.
    let st = s.clone();
    t.set(
        "providers",
        lua.create_function(move |lua, ()| {
            let list: Vec<_> = st.host.reg.borrow().providers.values().cloned().collect();
            let v = serde_json::to_value(list).map_err(mlua::Error::external)?;
            to_lua(lua, &v)
        })?,
    )?;
    let sh = s.host.shared.clone();
    t.set(
        "set_entries",
        // test.set_entries({ { "todo", { text = "x" } }, ... })
        lua.create_function(move |lua, list: Vec<Table>| {
            let mut entries = Vec::new();
            for pair in list {
                let k: String = pair.get(1)?;
                let v: Value = pair.get(2)?;
                entries.push((k, lua.from_value::<Json>(v)?));
            }
            if let Ok(mut s) = sh.write() {
                s.entries = entries;
            }
            Ok(())
        })?,
    )?;
    let sh = s.host.shared.clone();
    t.set(
        "set_flag",
        // test.set_flag("max_lines", "100"): as `--ext-flag max_lines=100`
        // gives it, so the value arrives as a string.
        lua.create_function(move |_, (name, value): (String, String)| {
            if let Ok(mut s) = sh.write() {
                s.flags.insert(name, value);
            }
            Ok(())
        })?,
    )?;
    // print goes to the case's output.
    let f = fake.clone();
    lua.globals().set(
        "print",
        lua.create_function(move |_, args: mlua::Variadic<Value>| {
            let parts: Vec<String> = args
                .iter()
                .map(|v| v.to_string().unwrap_or_else(|_| format!("{v:?}")))
                .collect();
            f.0.borrow_mut()
                .output
                .push(format!("print: {}", parts.join("\t")));
            Ok(())
        })?,
    )?;
    lua.globals().set("test", t)?;
    // Assertions and the virtual clock, in Lua.
    lua.load(TEST_LUA).set_name("=psi test").exec()
}

/// What `test.ui.answer` takes for each kind, for its error message.
fn answer_shape(kind: &str) -> &'static str {
    match kind {
        "confirm" => "pass true or false",
        "select" | "input" => "pass a string",
        "exec" => "pass a table like { code = 0, stdout = \"...\", stderr = \"\" }",
        "http" => "pass a table like { status = 200, body = \"...\" }",
        _ => "pass a string, number, boolean or table of those, not a function",
    }
}

const TEST_LUA: &str = r#"
local function show(v, depth)
  depth = depth or 0
  -- %q breaks lines with a backslash and a real newline; failures are
  -- reported by their first line, so keep the value on one.
  if type(v) == "string" then return (string.format("%q", v):gsub("\\\n", "\\n")) end
  if type(v) ~= "table" or depth > 3 then return tostring(v) end
  local keys = {}
  for k in pairs(v) do keys[#keys + 1] = k end
  table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
  local parts = {}
  for _, k in ipairs(keys) do parts[#parts + 1] = tostring(k) .. " = " .. show(v[k], depth + 1) end
  return "{ " .. table.concat(parts, ", ") .. " }"
end
local function same(a, b)
  if type(a) ~= type(b) then return false end
  if type(a) ~= "table" then return a == b end
  for k, v in pairs(a) do if not same(v, b[k]) then return false end end
  for k in pairs(b) do if a[k] == nil then return false end end
  return true
end
function test.eq(got, want, what)
  if not same(got, want) then
    error((what and (what .. ": ") or "") .. "expected " .. show(want) .. ", got " .. show(got), 2)
  end
end
function test.truthy(v, what)
  if not v then error((what or "expected a true value") .. ", got " .. show(v), 2) end
end
function test.contains(s, part, what)
  if type(s) ~= "string" or not s:find(part, 1, true) then
    error((what and (what .. ": ") or "") .. "expected " .. show(s) .. " to contain " .. show(part), 2)
  end
end
function test.neq(got, other, what)
  if same(got, other) then
    error((what and (what .. ": ") or "") .. "expected a value other than " .. show(other), 2)
  end
end
function test.falsy(v, what)
  if v then error((what or "expected false or nil") .. ", got " .. show(v), 2) end
end
function test.not_contains(s, part, what)
  if type(s) == "string" and s:find(part, 1, true) then
    error((what and (what .. ": ") or "") .. "expected " .. show(s) .. " not to contain " .. show(part), 2)
  end
end
function test.matches(s, pattern, what)
  if type(s) ~= "string" or not s:find(pattern) then
    error((what and (what .. ": ") or "") .. "expected " .. show(s) .. " to match the Lua pattern " .. show(pattern), 2)
  end
end
function test.errors(fn, part, what)
  local ok, err = pcall(fn)
  local prefix = what and (what .. ": ") or ""
  if ok then error(prefix .. "expected an error, but the function returned", 2) end
  err = tostring(err)
  if part and not err:find(part, 1, true) then
    error(prefix .. "expected an error containing " .. show(part) .. ", got " .. show(err), 2)
  end
  return err
end
-- A virtual clock: os.time and os.clock start at fixed values and move
-- only with test.clock.advance.
local now, cpu = 1700000000, 0
os.time = function(t) if t then return now end return now end
os.clock = function() return cpu end
test.clock = {
  advance = function(seconds) now = now + seconds; cpu = cpu + seconds end,
  set = function(t) now = t end,
}
"#;

/// Every extension under `dir` that has tests (`psi ext test examples/extensions`).
pub fn run_all(dir: &Path) -> Vec<(PathBuf, TestReport)> {
    let mut out = Vec::new();
    for s in load::discover(dir).into_iter().flatten() {
        if let crate::Source::Lua(p) = s {
            let target = if p.file_name().is_some_and(|n| n == "init.lua") {
                p.parent().map(Path::to_owned).unwrap_or(p)
            } else {
                p
            };
            if layout(&target).is_ok_and(|(_, t)| !t.is_empty()) {
                let r = run(&target);
                out.push((target, r));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passing_and_failing_cases_with_their_output() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("init.lua"),
            r#"return { name = "t", version = "1", description = "d", setup = function(psi)
                 psi.register_command("hello", { description = "greet", handler = function(args, ctx)
                   print("greeting " .. args)
                   if ctx.ui.confirm("Greet?", args) then psi.ui.notify("hello " .. args) end
                 end })
                 psi.on("tool_call", function(ev) if ev.args.command == "x" then return { block = true, reason = "no x" } end end)
               end }"#,
        )
        .unwrap();
        std::fs::write(
            d.path().join("hello_test.lua"),
            r#"
            test.case("greets when the user agrees", function()
              test.ui.answer("confirm", true)
              test.command("/hello", "bob")
              test.eq(#test.requests("notify"), 1)
              test.eq(test.requests("notify")[1].message, "hello bob")
            end)
            test.case("blocks x", function()
              test.truthy(test.emit("tool_call", { id = "1", name = "bash", args = { command = "x" } }).blocked)
            end)
            test.case("fails with a clear message", function()
              test.command("/hello", "ann")
              test.eq(test.requests("notify")[1], { kind = "notify" })
            end)
            test.case("virtual clock", function()
              local t = os.time(); test.clock.advance(60); test.eq(os.time() - t, 60)
            end)
            "#,
        )
        .unwrap();
        let r = run(d.path());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let outcomes: Vec<bool> = r.cases.iter().map(|c| c.error.is_none()).collect();
        assert_eq!(outcomes, [true, true, false, true], "{}", r.render(true));
        let fail = &r.cases[2];
        let e = fail.error.as_deref().unwrap();
        assert!(e.contains("expected { kind = \"notify\" }, got nil"), "{e}");
        assert!(e.contains("hello_test.lua:13"), "{e}");
        assert_eq!(fail.output, ["print: greeting ann"]);
        let text = r.render(false);
        assert!(
            text.contains("FAIL  hello_test.lua: fails with a clear message"),
            "{text}"
        );
        assert!(text.contains("| print: greeting ann"), "{text}");
        assert!(text.ends_with("3 passed, 1 failed\n"), "{text}");
    }

    #[test]
    fn commands_and_requests_get_scripted_answers() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("init.lua"),
            r#"return { name = "g", version = "1", description = "d", setup = function(psi)
                 psi.register_command("branch", { description = "b", handler = function()
                   local r = psi.exec({ "git", "branch", "--show-current" })
                   local h = psi.http.get("https://example.com/x")
                   psi.ui.notify(r.stdout .. h.json().v)
                 end })
               end }"#,
        )
        .unwrap();
        std::fs::write(
            d.path().join("g_test.lua"),
            r#"
            test.case("answers", function()
              test.ui.answer("exec", { code = 0, stdout = "main " })
              test.ui.answer("http", { status = 200, body = '{"v":"ok"}' })
              test.command("/branch", "")
              test.eq(test.requests("notify")[1].message, "main ok")
              test.eq(test.requests("exec")[1].argv, { "git", "branch", "--show-current" })
            end)
            test.case("no answer", function()
              test.command("/branch", "")
            end)
            "#,
        )
        .unwrap();
        let r = run(d.path());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r.cases[0].error.is_none(), "{}", r.render(true));
        let text = r.render(true);
        assert!(
            text.contains("test: no answer for git branch --show-current"),
            "{text}"
        );
    }

    #[test]
    fn a_multi_line_mismatch_shows_both_values_on_one_line() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("init.lua"),
            r#"return { name = "m", version = "1", description = "d", setup = function() end }"#,
        )
        .unwrap();
        std::fs::write(
            d.path().join("m_test.lua"),
            r#"test.case("lines", function() test.eq("a\nb", "a\nc") end)"#,
        )
        .unwrap();
        let r = run(d.path());
        assert_eq!(r.cases.len(), 1, "{}", r.render(true));
        let e = r.cases[0].error.as_deref().unwrap();
        assert!(e.ends_with(r#"expected "a\nc", got "a\nb""#), "{e}");
    }

    #[test]
    fn the_extra_assertions_pass_and_fail_with_clear_messages() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("init.lua"),
            r#"return { name = "a", version = "1", description = "d", setup = function() end }"#,
        )
        .unwrap();
        std::fs::write(
            d.path().join("a_test.lua"),
            r#"
            test.case("pass", function()
              test.neq(1, 2); test.neq({ a = 1 }, { a = 2 })
              test.falsy(nil); test.falsy(false)
              test.not_contains("fix-parse ok", "fail")
              test.matches("go 1.27", "^go %d+%.%d+$")
              test.contains("fix-parse", "x-p")
              local e = test.errors(function() error("boom here") end, "boom")
              test.contains(e, "boom here")
            end)
            test.case("neq", function() test.neq({ 1 }, { 1 }) end)
            test.case("falsy", function() test.falsy(0) end)
            test.case("not_contains", function() test.not_contains("abc", "b") end)
            test.case("matches", function() test.matches("abc", "^%d") end)
            test.case("errors", function() test.errors(function() end) end)
            test.case("errors text", function() test.errors(function() error("x") end, "qqq") end)
            test.case("answer", function() test.ui.answer("exec", function() end) end)
            "#,
        )
        .unwrap();
        let r = run(d.path());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let errors: Vec<&str> = r
            .cases
            .iter()
            .map(|c| c.error.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(errors[0], "", "{}", r.render(true));
        let want = [
            "expected a value other than { 1 = 1 }",
            "expected false or nil, got 0",
            "expected \"abc\" not to contain \"b\"",
            "expected \"abc\" to match the Lua pattern \"^%d\"",
            "expected an error, but the function returned",
            "expected an error containing \"qqq\"",
            "test.ui.answer(\"exec\", ...): the answer must be a plain value",
        ];
        for (got, want) in errors[1..].iter().zip(want) {
            assert!(got.contains(want), "{got:?} lacks {want:?}");
        }
        assert!(errors[7].contains("{ code = 0, stdout"), "{}", errors[7]);
    }

    #[test]
    fn flags_set_per_case() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("init.lua"),
            r#"return { name = "f", version = "1", description = "d", setup = function(psi)
                 psi.register_flag("n", { description = "a number", default = 3 })
                 psi.register_command("n", { description = "show", handler = function()
                   psi.ui.notify(tostring(psi.get_flag("n")))
                 end })
               end }"#,
        )
        .unwrap();
        std::fs::write(
            d.path().join("f_test.lua"),
            r#"
            test.case("set", function()
              test.set_flag("n", 7)
              test.command("n")
              test.eq(test.requests("notify")[1].message, "7")
            end)
            test.case("default again", function()
              test.command("n")
              test.eq(test.requests("notify")[1].message, "3")
            end)
            "#,
        )
        .unwrap();
        let r = run(d.path());
        assert!(r.passed(), "{}", r.render(true));
    }

    #[test]
    fn setup_errors_are_reported() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("init.lua"), "return { name = 'x', version = '1', description = 'd', setup = function() error('boom') end }").unwrap();
        std::fs::write(d.path().join("x_test.lua"), "").unwrap();
        let r = run(d.path());
        assert!(!r.passed());
        assert!(
            r.errors[0].contains("setup failed: ") && r.errors[0].contains("boom"),
            "{:?}",
            r.errors
        );
    }
}
