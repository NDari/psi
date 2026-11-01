//! `psi ext check`, the extension-author lint (psi-plan.md §9.6, D-15).
//!
//! It checks the shape, a script tool's `tool.json`, Lua syntax, a static pass
//! against the API's signature tables, a dry load against [`RecordingHost`],
//! and `lua-language-server --check` when it is installed. Nothing is printed:
//! the [`Report`] says what was found.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use full_moon::ast::{
    self, Call, Expression, Field, FunctionArgs, FunctionBody, Index, LastStmt, Parameter, Prefix,
    Stmt, Suffix, Var,
};
use full_moon::node::Node;
use full_moon::tokenizer::{Position, Symbol, TokenReference, TokenType};
use mlua::{Function, HookTriggers, Lua, MultiValue, Table, Value, VmState};
use psi_ext_api::types::{self, Ty};
use psi_ext_api::{EVENT_NAMES, ExtHost, FUNCTIONS, FnSig, LUALS_STUBS, TYPES};
use serde_json::Value as Json;

// ---- Report -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        })
    }
}

/// One finding. `line` 0 means the whole file (no position).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub diags: Vec<Diag>,
    /// What a dry load registered, for the summary: "tool lint_tool",
    /// "command /todo", "on tool_call", ...
    pub registered: Vec<String>,
    /// Everything printed, logged or notified during the dry load, in order.
    pub output: Vec<String>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, s: Severity) -> usize {
        self.diags.iter().filter(|d| d.severity == s).count()
    }

    /// `file:line:col: error: message` lines, what the load printed, then a
    /// summary line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for d in &self.diags {
            let file = d.file.display();
            if d.line == 0 {
                out += &format!("{file}: {}: {}\n", d.severity, d.message);
            } else {
                out += &format!(
                    "{file}:{}:{}: {}: {}\n",
                    d.line, d.col, d.severity, d.message
                );
            }
        }
        for o in &self.output {
            out += &format!("output: {o}\n");
        }
        let (e, w) = (self.errors(), self.warnings());
        out += &format!("{e} {}, {w} {}", plural(e, "error"), plural(w, "warning"));
        if !self.registered.is_empty() {
            out += &format!("; registered {}", self.registered.join(", "));
        }
        out.push('\n');
        out
    }

    fn add(&mut self, file: &Path, (line, col): (usize, usize), severity: Severity, msg: String) {
        self.diags.push(Diag {
            file: file.to_owned(),
            line,
            col,
            severity,
            message: msg,
        });
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.into()
    } else {
        format!("{word}s")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Run `lua-language-server --check` when it is on PATH.
    pub luals: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { luals: true }
    }
}

/// Lints an extension: a `name.lua` file, a directory with `init.lua`, or a
/// script-tool directory with `tool.json`.
pub fn check(path: &Path) -> Report {
    check_with(path, Options::default())
}

pub fn check_with(path: &Path, opts: Options) -> Report {
    let mut r = Report::default();
    if path.is_file() && path.extension().is_some_and(|e| e == "lua") {
        lint_lua(path, &[path.to_owned()], None, opts, &mut r);
    } else if path.is_dir() && path.join("init.lua").is_file() {
        if path.join("tool.json").is_file() {
            let msg = "tool.json is ignored: init.lua makes this a Lua extension".into();
            r.add(&path.join("tool.json"), (0, 0), Severity::Warning, msg);
        }
        let main = path.join("init.lua");
        let mut files = vec![main.clone()];
        lua_files(path, &mut files, 0);
        files[1..].sort();
        lint_lua(&main, &files, Some(path), opts, &mut r);
    } else if path.is_dir() && path.join("tool.json").is_file() {
        script_tool(path, &mut r);
    } else {
        let what = if path.exists() {
            "is not an extension"
        } else {
            "does not exist"
        };
        let msg = format!(
            "{what}; psi ext check takes a name.lua file, a directory with init.lua, \
             or a script tool directory with tool.json"
        );
        r.add(path, (0, 0), Severity::Error, msg);
    }
    r.diags
        .sort_by(|a, b| (&a.file, a.line, a.col).cmp(&(&b.file, b.line, b.col)));
    r
}

/// The other `.lua` files under `dir` (not `init.lua`, tests or hidden dirs).
fn lua_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() && depth < 8 {
            lua_files(&p, out, depth + 1);
        } else if name.ends_with(".lua")
            && !name.ends_with("_test.lua")
            && !(depth == 0 && name == "init.lua")
        {
            out.push(p);
        }
    }
}

// ---- Names and suggestions --------------------------------------------------

/// A tool name the model can use (and a script tool's name).
fn valid_tool_name(n: &str) -> bool {
    n.starts_with(|c: char| c.is_ascii_lowercase())
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// An extension's manifest name; `-` is allowed too (psi-plan.md §9.3 uses `git-guard`).
fn valid_ext_name(n: &str) -> bool {
    n.starts_with(|c: char| c.is_ascii_lowercase())
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The closest candidate, if it is close enough to be a likely typo.
fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let max = 2.max(name.len() / 3);
    candidates
        .into_iter()
        .map(|c| (edit_distance(name, c), c))
        .filter(|(d, _)| *d <= max)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

/// `; did you mean X?` or nothing.
fn did_you_mean(s: Option<&str>, prefix: &str) -> String {
    s.map(|s| format!("; did you mean {prefix}{s}?"))
        .unwrap_or_default()
}

// ---- Script tools (D-22) ----------------------------------------------------

const PARAM_TYPES: &[&str] = &["string", "integer", "number", "boolean", "array", "object"];
const TOOL_KEYS: &[&str] = &[
    "name",
    "description",
    "params",
    "required",
    "always_ask",
    "read_only",
    "command",
];

fn script_tool(dir: &Path, r: &mut Report) {
    let file = dir.join("tool.json");
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) => return r.add(&file, (0, 0), Severity::Error, format!("cannot read: {e}")),
    };
    let json: Json = match serde_json::from_str(&text) {
        Ok(j) => j,
        Err(e) => {
            let msg = format!("not valid JSON: {e}");
            return r.add(&file, (e.line(), e.column()), Severity::Error, msg);
        }
    };
    let Json::Object(m) = json else {
        let msg = "tool.json must be a JSON object { name, description, params, command }";
        return r.add(&file, (1, 1), Severity::Error, msg.into());
    };
    let at = |key: &str| json_key_pos(&text, key);
    let mut err = |key: &str, msg: String| r.add(&file, at(key), Severity::Error, msg);
    // Warnings wait until `err` is done with `r`.
    let mut warns: Vec<(String, String)> = Vec::new();

    match m.get("name") {
        Some(Json::String(n)) if valid_tool_name(n) => {}
        Some(Json::String(n)) => err(
            "name",
            format!("name '{n}' must be lowercase letters, digits and _, starting with a letter"),
        ),
        Some(v) => err(
            "name",
            format!("name expected string, got {}", json_type(v)),
        ),
        None => err("", "missing 'name' (the tool name the model calls)".into()),
    }
    match m.get("description") {
        Some(Json::String(d)) if !d.trim().is_empty() => {}
        Some(Json::String(_)) => err("description", "description is empty".into()),
        Some(v) => err(
            "description",
            format!("description expected string, got {}", json_type(v)),
        ),
        None => err(
            "",
            "missing 'description' (what the tool does, for the model)".into(),
        ),
    }
    let mut params: Vec<&str> = Vec::new();
    match m.get("params") {
        None => {}
        Some(Json::Object(ps)) => {
            for (pname, p) in ps {
                params.push(pname);
                let Json::Object(p) = p else {
                    let got = json_type(p);
                    err(
                        pname,
                        format!(
                            "params.{pname} expected an object {{type, description}}, got {got}"
                        ),
                    );
                    continue;
                };
                match p.get("type") {
                    Some(Json::String(t)) if PARAM_TYPES.contains(&t.as_str()) => {}
                    Some(Json::String(t)) => err(
                        pname,
                        format!(
                            "params.{pname}: type '{t}' is not one of {}",
                            PARAM_TYPES.join(", ")
                        ),
                    ),
                    _ => err(
                        pname,
                        format!("params.{pname} needs a type ({})", PARAM_TYPES.join(", ")),
                    ),
                }
                if !matches!(p.get("description"), Some(Json::String(_))) {
                    err(
                        pname,
                        format!("params.{pname} needs a description (a string)"),
                    );
                }
                for k in p.keys().filter(|k| *k != "type" && *k != "description") {
                    let s = did_you_mean(suggest(k, ["type", "description"]), "");
                    let msg = format!("params.{pname}: unknown key '{k}'{s}");
                    warns.push((k.clone(), msg));
                }
            }
        }
        Some(v) => err(
            "params",
            format!("params expected an object, got {}", json_type(v)),
        ),
    }
    match m.get("required") {
        None => {}
        Some(Json::Array(names)) => {
            for n in names {
                match n {
                    Json::String(n) if params.contains(&n.as_str()) => {}
                    Json::String(n) => {
                        let s = did_you_mean(suggest(n, params.iter().copied()), "");
                        err(
                            "required",
                            format!("required names '{n}', which is not in params{s}"),
                        );
                    }
                    v => err(
                        "required",
                        format!("required expected string[], got {}", json_type(v)),
                    ),
                }
            }
        }
        Some(v) => err(
            "required",
            format!("required expected string[], got {}", json_type(v)),
        ),
    }
    for key in ["always_ask", "read_only"] {
        if let Some(v) = m.get(key).filter(|v| !v.is_boolean()) {
            err(key, format!("{key} expected boolean, got {}", json_type(v)));
        }
    }
    match m.get("command") {
        Some(Json::String(c)) => check_command(dir, c, &mut err),
        Some(Json::Array(a)) => match a.first() {
            Some(Json::String(c)) if a.iter().all(Json::is_string) => {
                check_command(dir, c, &mut err)
            }
            Some(_) => err(
                "command",
                "command must be a string or an array of strings".into(),
            ),
            None => err("command", "command is an empty array".into()),
        },
        Some(v) => err(
            "command",
            format!("command expected a string or array, got {}", json_type(v)),
        ),
        None => err(
            "",
            "missing 'command' (a file in this directory, or [program, args...])".into(),
        ),
    }
    for k in m.keys().filter(|k| !TOOL_KEYS.contains(&k.as_str())) {
        let s = did_you_mean(suggest(k, TOOL_KEYS.iter().copied()), "");
        warns.push((k.clone(), format!("unknown key '{k}'{s}")));
    }
    for (k, msg) in warns {
        r.add(&file, at(&k), Severity::Warning, msg);
    }
    if let Some(Json::String(n)) = m.get("name").filter(|_| r.errors() == 0) {
        r.registered.push(format!("tool {n}"));
    }
}

/// The program must be an executable in the directory or a bare name on PATH,
/// resolved as the loader does (`psi-ext` `ScriptTool::argv`).
fn check_command(dir: &Path, c: &str, err: &mut impl FnMut(&str, String)) {
    let local = dir.join(c);
    if c.contains('/') || local.exists() {
        if !local.is_file() {
            err(
                "command",
                format!("command '{c}' does not exist in {}", dir.display()),
            );
        } else if !is_executable(&local) {
            err(
                "command",
                format!("command '{c}' is not executable (chmod +x {c})"),
            );
        }
    } else if find_on_path(c).is_none() {
        err(
            "command",
            format!("command '{c}' is not in the tool directory or on PATH"),
        );
    }
}

fn json_type(v: &Json) -> &'static str {
    match v {
        Json::Null => "null",
        Json::Bool(_) => "boolean",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
    }
}

/// Where `"key":` first appears in the text (good enough to point at a key);
/// the start for `""` (a missing key).
fn json_key_pos(text: &str, key: &str) -> (usize, usize) {
    if key.is_empty() {
        return (1, 1);
    }
    let quoted = format!("\"{key}\"");
    let mut from = 0;
    while let Some(i) = text[from..].find(&quoted) {
        let start = from + i;
        let after = text[start + quoted.len()..].trim_start();
        if after.starts_with(':') {
            let line = text[..start].matches('\n').count() + 1;
            let col = start - text[..start].rfind('\n').map_or(0, |n| n + 1) + 1;
            return (line, col);
        }
        from = start + quoted.len();
    }
    (1, 1)
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file() && is_executable(p))
}

// ---- Lua: syntax, static pass, dry load, luals ------------------------------

/// What the main file's returned manifest looks like, from the AST.
#[derive(Debug, Default)]
struct MainInfo {
    /// The `return` statement.
    ret: Option<(usize, usize)>,
    /// Each manifest field's key.
    fields: HashMap<String, (usize, usize)>,
    setup: Option<SetupFn>,
}

#[derive(Debug)]
struct SetupFn {
    /// [`body_key`] of the function.
    key: usize,
    /// The line its definition starts on (where a `---@param` goes).
    line: usize,
    /// The name of its first parameter, the `psi` table.
    param: Option<String>,
}

fn lint_lua(main: &Path, files: &[PathBuf], dir: Option<&Path>, opts: Options, r: &mut Report) {
    let mut ok = true;
    let mut info = MainInfo::default();
    let mut main_src = String::new();
    for f in files {
        let src = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                r.add(f, (0, 0), Severity::Error, format!("cannot read: {e}"));
                ok = false;
                continue;
            }
        };
        let parsed = full_moon::parse_fallible(&src, full_moon::LuaVersion::lua54());
        if let Some(e) = parsed.errors().first() {
            let (start, _) = e.range();
            let mut msg = format!("syntax error: {}", strip_range(&e.error_message()));
            if src
                .get(start.bytes()..)
                .is_some_and(|s| s.starts_with("!="))
            {
                msg += " (Lua writes 'not equal' as ~=)";
            }
            r.add(f, pos(start), Severity::Error, msg);
            ok = false;
            continue;
        }
        let ast = parsed.ast();
        let file_info = if f == main {
            main_info(ast.nodes())
        } else {
            MainInfo::default()
        };
        let mut lint = Lint::new(f);
        if let Some(s) = &file_info.setup {
            lint.fn_aliases.insert(s.key, vec![(0, Alias::Psi)]);
        }
        lint.run(ast.nodes());
        r.diags.extend(lint.diags);
        if f == main {
            info = file_info;
            main_src = src;
        }
    }
    if !ok {
        return;
    }
    dry_load(main, &main_src, dir, &info, r);
    if opts.luals {
        luals(main, files, dir, &info, r);
    }
}

/// Drops the ` (4:10 to 4:11)` that full_moon's tokenizer errors end with.
fn strip_range(msg: &str) -> &str {
    match msg.rsplit_once(" (") {
        Some((head, tail))
            if tail.ends_with(')')
                && tail
                    .chars()
                    .all(|c| c.is_ascii_digit() || " :to)".contains(c)) =>
        {
            head
        }
        _ => msg,
    }
}

fn pos(p: Position) -> (usize, usize) {
    (p.line(), p.character())
}

fn tok_pos(t: &TokenReference) -> (usize, usize) {
    pos(t.token().start_position())
}

fn node_pos(n: &impl Node) -> (usize, usize) {
    n.start_position().map_or((0, 0), pos)
}

fn name_of(t: &TokenReference) -> String {
    t.token().to_string()
}

/// Identifies a function body: the byte offset of its `(`.
fn body_key(b: &FunctionBody) -> usize {
    b.parameters_parentheses()
        .tokens()
        .0
        .token()
        .start_position()
        .bytes()
}

fn first_param(b: &FunctionBody) -> Option<String> {
    match b.parameters().iter().next() {
        Some(Parameter::Name(t)) => Some(name_of(t)),
        _ => None,
    }
}

fn setup_fn(b: &FunctionBody, line: usize) -> SetupFn {
    SetupFn {
        key: body_key(b),
        line,
        param: first_param(b),
    }
}

fn strip_parens(mut e: &Expression) -> &Expression {
    while let Expression::Parentheses { expression, .. } = e {
        e = expression;
    }
    e
}

/// Finds the manifest the file returns and its `setup` function: a table
/// constructor with `setup = function(psi)` or `setup = name`, or a returned
/// local `M` with `function M.setup(psi)` or `M.setup = function(psi)`.
fn main_info(block: &ast::Block) -> MainInfo {
    let mut info = MainInfo::default();
    let Some(LastStmt::Return(ret)) = block.last_stmt() else {
        return info;
    };
    info.ret = Some(tok_pos(ret.token()));
    let Some(first) = ret.returns().iter().next() else {
        return info;
    };
    let stmts: Vec<&Stmt> = block.stmts().collect();
    match strip_parens(first) {
        Expression::TableConstructor(t) => {
            for f in t.fields().iter() {
                let Field::NameKey { key, value, .. } = f else {
                    continue;
                };
                let k = name_of(key);
                info.fields.insert(k.clone(), tok_pos(key));
                if k != "setup" {
                    continue;
                }
                match strip_parens(value) {
                    Expression::Function(func) => {
                        info.setup = Some(setup_fn(func.body(), tok_pos(key).0));
                    }
                    Expression::Var(Var::Name(n)) => info.setup = local_fn(&stmts, &name_of(n)),
                    _ => {}
                }
            }
        }
        Expression::Var(Var::Name(m)) => info.setup = module_setup(&stmts, &name_of(m)),
        _ => {}
    }
    info
}

/// `local function name(...)` or `local name = function(...)`, at the top level.
fn local_fn(stmts: &[&Stmt], name: &str) -> Option<SetupFn> {
    stmts.iter().rev().find_map(|s| match s {
        Stmt::LocalFunction(f) if name_of(f.name()) == name => {
            Some(setup_fn(f.body(), tok_pos(f.local_token()).0))
        }
        Stmt::LocalAssignment(a) => {
            let i = a.names().iter().position(|n| name_of(n) == name)?;
            match a.expressions().iter().nth(i).map(strip_parens) {
                Some(Expression::Function(func)) => {
                    Some(setup_fn(func.body(), tok_pos(a.local_token()).0))
                }
                _ => None,
            }
        }
        _ => None,
    })
}

/// `function M.setup(...)` or `M.setup = function(...)`, at the top level.
fn module_setup(stmts: &[&Stmt], m: &str) -> Option<SetupFn> {
    stmts.iter().rev().find_map(|s| match s {
        Stmt::FunctionDeclaration(f) => {
            let names: Vec<String> = f.name().names().iter().map(name_of).collect();
            (f.name().method_name().is_none() && names == [m, "setup"])
                .then(|| setup_fn(f.body(), tok_pos(f.function_token()).0))
        }
        Stmt::Assignment(a) => {
            let i = a.variables().iter().position(|v| {
                let Var::Expression(ve) = v else { return false };
                let Prefix::Name(n) = ve.prefix() else {
                    return false;
                };
                let suffixes: Vec<&Suffix> = ve.suffixes().collect();
                name_of(n) == m
                    && matches!(suffixes.as_slice(),
                        [Suffix::Index(Index::Dot { name, .. })] if name_of(name) == "setup")
            })?;
            match a.expressions().iter().nth(i).map(strip_parens) {
                Some(Expression::Function(func)) => Some(setup_fn(func.body(), node_pos(a).0)),
                _ => None,
            }
        }
        _ => None,
    })
}

// ---- Static pass --------------------------------------------------------------

/// Lua 5.4's standard globals.
const STD_GLOBALS: &[&str] = &[
    "_G",
    "_VERSION",
    "_ENV",
    "assert",
    "collectgarbage",
    "coroutine",
    "debug",
    "dofile",
    "error",
    "getmetatable",
    "io",
    "ipairs",
    "load",
    "loadfile",
    "math",
    "next",
    "os",
    "package",
    "pairs",
    "pcall",
    "print",
    "rawequal",
    "rawget",
    "rawlen",
    "rawset",
    "require",
    "select",
    "setmetatable",
    "string",
    "table",
    "tonumber",
    "tostring",
    "type",
    "utf8",
    "warn",
    "xpcall",
];

/// Globals an extension may assign without a warning.
const GLOBAL_ALLOW: &[&str] = &[];

/// Lua 5.1 globals that are gone in 5.4, and what to use instead.
const OLD_GLOBALS: &[(&str, &str)] = &[
    ("unpack", "unpack is Lua 5.1; use table.unpack"),
    ("loadstring", "loadstring is Lua 5.1; use load"),
    ("setfenv", "setfenv is Lua 5.1; Lua 5.4 has _ENV instead"),
    ("getfenv", "getfenv is Lua 5.1; Lua 5.4 has _ENV instead"),
];

/// What a name or expression refers to, as far as the API goes.
#[derive(Debug, Clone, Copy)]
enum Alias {
    /// The `psi` table.
    Psi,
    /// `psi.ui`, `psi.events` or `psi.style`.
    Sub(&'static str),
    /// A handler's `ctx`.
    Ctx,
    Func(&'static FnSig),
}

impl Alias {
    fn path(self) -> String {
        match self {
            Alias::Psi => "psi".into(),
            Alias::Sub(s) => format!("psi.{s}"),
            Alias::Ctx => "ctx".into(),
            Alias::Func(f) => format!("psi.{}", f.name),
        }
    }
}

fn function(name: &str) -> Option<&'static FnSig> {
    FUNCTIONS.iter().find(|f| f.name == name)
}

fn type_sig(name: &str) -> Option<&'static psi_ext_api::TypeSig> {
    TYPES.iter().find(|t| t.name == name)
}

/// `alias.name`: `Ok(Some)` for an API member, `Ok(None)` for a plain value,
/// `Err(candidates)` when there is no such member.
fn member(alias: Alias, name: &str) -> Result<Option<Alias>, Vec<&'static str>> {
    match alias {
        Alias::Psi => {
            if let Some(f) = FUNCTIONS.iter().find(|f| f.name == name) {
                return Ok(Some(Alias::Func(f)));
            }
            let mut cands = Vec::new();
            for f in FUNCTIONS {
                match f.name.split_once('.') {
                    Some((sub, _)) if sub == name => return Ok(Some(Alias::Sub(sub))),
                    Some((sub, _)) => cands.push(sub),
                    None => cands.push(f.name),
                }
            }
            Err(cands)
        }
        Alias::Sub(s) => {
            let full = format!("{s}.{name}");
            if let Some(f) = function(&full) {
                return Ok(Some(Alias::Func(f)));
            }
            Err(FUNCTIONS
                .iter()
                .filter_map(|f| f.name.strip_prefix(s)?.strip_prefix('.'))
                .collect())
        }
        Alias::Ctx => {
            let fields = type_sig("Context").map_or(&[][..], |t| t.fields);
            match name {
                "ui" => Ok(Some(Alias::Sub("ui"))),
                "execute_tool" => Ok(function("execute_tool").map(Alias::Func)),
                _ if fields.iter().any(|f| f.name == name) => Ok(None),
                _ => Err(fields.iter().map(|f| f.name).collect()),
            }
        }
        Alias::Func(_) => Ok(None),
    }
}

/// The obvious Lua type of a literal expression.
#[derive(Debug, Clone, PartialEq)]
enum Lit {
    /// The value, when it is a plain string literal.
    Str(Option<String>),
    /// Whether it is an integer, when known.
    Num(Option<bool>),
    Bool,
    Nil,
    Table,
    Func,
}

impl Lit {
    fn type_name(&self) -> &'static str {
        match self {
            Lit::Str(_) => "string",
            Lit::Num(_) => "number",
            Lit::Bool => "boolean",
            Lit::Nil => "nil",
            Lit::Table => "table",
            Lit::Func => "function",
        }
    }
}

fn string_value(t: &TokenReference) -> Option<String> {
    match t.token_type() {
        TokenType::StringLiteral { literal, .. } => Some(literal.to_string()),
        _ => None,
    }
}

fn number_is_integer(text: &str) -> Option<bool> {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("0x") {
        return (!lower.contains(['.', 'p'])).then_some(true);
    }
    if !lower.contains(['.', 'e']) {
        return Some(true);
    }
    lower.parse::<f64>().ok().map(|n| n.fract() == 0.0)
}

fn literal(e: &Expression) -> Option<Lit> {
    use ast::{BinOp, UnOp};
    Some(match strip_parens(e) {
        Expression::String(t) => Lit::Str(string_value(t)),
        Expression::Number(t) => Lit::Num(number_is_integer(&name_of(t))),
        Expression::Symbol(t) => match t.token_type() {
            TokenType::Symbol {
                symbol: Symbol::True | Symbol::False,
            } => Lit::Bool,
            TokenType::Symbol {
                symbol: Symbol::Nil,
            } => Lit::Nil,
            _ => return None,
        },
        Expression::TableConstructor(_) => Lit::Table,
        Expression::Function(_) => Lit::Func,
        Expression::UnaryOperator { unop, expression } => match unop {
            UnOp::Not(_) => Lit::Bool,
            UnOp::Hash(_) => Lit::Num(Some(true)),
            UnOp::Minus(_) => match literal(expression) {
                Some(Lit::Num(i)) => Lit::Num(i),
                _ => Lit::Num(None),
            },
            _ => Lit::Num(None),
        },
        Expression::BinaryOperator { binop, .. } => match binop {
            BinOp::TwoDots(_) => Lit::Str(None),
            BinOp::TwoEqual(_)
            | BinOp::TildeEqual(_)
            | BinOp::LessThan(_)
            | BinOp::LessThanEqual(_)
            | BinOp::GreaterThan(_)
            | BinOp::GreaterThanEqual(_) => Lit::Bool,
            BinOp::And(_) | BinOp::Or(_) => return None,
            _ => Lit::Num(None),
        },
        _ => return None,
    })
}

/// Named types resolve to their table shape, except `EventName`.
fn is_event_name(name: &str) -> bool {
    type_sig(name).is_some_and(|t| t.alias == Some("events"))
}

/// Whether a value of literal type `l` can be passed where `ty` is declared,
/// with the same leniency as the runtime glue.
fn accepts(ty: &Ty, l: &Lit) -> bool {
    match ty {
        Ty::Any => true,
        Ty::Optional(t) => *l == Lit::Nil || accepts(t, l),
        Ty::Nil => *l == Lit::Nil,
        Ty::String => matches!(l, Lit::Str(_)),
        Ty::Literals(ls) => match l {
            Lit::Str(Some(s)) => ls.contains(s),
            Lit::Str(None) => true,
            _ => false,
        },
        Ty::Integer => matches!(l, Lit::Num(Some(true) | None)),
        Ty::Number => matches!(l, Lit::Num(_)),
        Ty::Boolean => *l == Lit::Bool,
        Ty::Named(n) if is_event_name(n) => match l {
            Lit::Str(Some(s)) => EVENT_NAMES.contains(&s.as_str()),
            Lit::Str(None) => true,
            _ => false,
        },
        Ty::Table | Ty::List(_) | Ty::Map(..) | Ty::Named(_) => *l == Lit::Table,
        Ty::Function { .. } => *l == Lit::Func,
        Ty::Union(ts) => ts.iter().any(|t| accepts(t, l)),
    }
}

fn unwrap_optional(ty: &Ty) -> &Ty {
    match ty {
        Ty::Optional(t) => t,
        t => t,
    }
}

fn parse_ty(s: &str) -> Ty {
    types::parse(s).unwrap_or(Ty::Any)
}

fn is_numeric_string(s: &str) -> bool {
    let s = s.trim();
    s.parse::<f64>().is_ok()
        || s.strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .is_some_and(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()))
}

struct Lint<'a> {
    file: &'a Path,
    diags: Vec<Diag>,
    /// Innermost last; each local's alias, if it refers to the API.
    scopes: Vec<HashMap<String, Option<Alias>>>,
    /// [`body_key`] to the parameters (by index) that refer to the API.
    fn_aliases: HashMap<usize, Vec<(usize, Alias)>>,
    assigned_globals: HashSet<String>,
    global_reads: Vec<(String, (usize, usize))>,
}

impl<'a> Lint<'a> {
    fn new(file: &'a Path) -> Self {
        Self {
            file,
            diags: Vec::new(),
            scopes: Vec::new(),
            fn_aliases: HashMap::new(),
            assigned_globals: HashSet::new(),
            global_reads: Vec::new(),
        }
    }

    fn diag(&mut self, at: (usize, usize), severity: Severity, message: String) {
        self.diags.push(Diag {
            file: self.file.to_owned(),
            line: at.0,
            col: at.1,
            severity,
            message,
        });
    }

    fn run(&mut self, block: &ast::Block) {
        self.block(block);
        let reads = std::mem::take(&mut self.global_reads);
        for (name, at) in reads {
            if self.assigned_globals.contains(&name) {
                continue;
            }
            let cands = STD_GLOBALS.iter().copied().chain(["psi"]);
            let s = did_you_mean(suggest(&name, cands), "");
            self.diag(at, Severity::Warning, format!("unknown global '{name}'{s}"));
        }
    }

    fn lookup(&self, name: &str) -> Option<Option<Alias>> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn declare(&mut self, name: String, alias: Option<Alias>) {
        if let Some(s) = self.scopes.last_mut() {
            s.insert(name, alias);
        }
    }

    fn block(&mut self, b: &ast::Block) {
        self.scopes.push(HashMap::new());
        self.stmts(b);
        self.scopes.pop();
    }

    /// The statements of `b` in the current scope.
    fn stmts(&mut self, b: &ast::Block) {
        for s in b.stmts() {
            self.stmt(s);
        }
        if let Some(LastStmt::Return(r)) = b.last_stmt() {
            for e in r.returns().iter() {
                self.expr(e);
            }
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Assignment(a) => {
                for e in a.expressions().iter() {
                    self.expr(e);
                }
                for v in a.variables().iter() {
                    self.assign_target(v);
                }
            }
            Stmt::LocalAssignment(a) => {
                let aliases: Vec<Option<Alias>> =
                    a.expressions().iter().map(|e| self.expr(e)).collect();
                for (i, n) in a.names().iter().enumerate() {
                    self.declare(name_of(n), aliases.get(i).copied().flatten());
                }
            }
            Stmt::LocalFunction(f) => {
                self.declare(name_of(f.name()), None);
                self.body(f.body(), false);
            }
            Stmt::FunctionDeclaration(f) => {
                let names: Vec<&TokenReference> = f.name().names().iter().collect();
                if let [only] = names.as_slice()
                    && f.name().method_name().is_none()
                {
                    let n = name_of(only);
                    if self.lookup(&n).is_none() {
                        self.global_assignment(n, tok_pos(only), true);
                    }
                } else if let Some(first) = names.first() {
                    self.read_name(first);
                }
                self.body(f.body(), f.name().method_name().is_some());
            }
            Stmt::FunctionCall(c) => {
                let suffixes: Vec<&Suffix> = c.suffixes().collect();
                self.chain(c.prefix(), &suffixes);
            }
            Stmt::Do(d) => self.block(d.block()),
            Stmt::While(w) => {
                self.expr(w.condition());
                self.block(w.block());
            }
            Stmt::Repeat(r) => {
                // `until` sees the block's locals.
                self.scopes.push(HashMap::new());
                self.stmts(r.block());
                self.expr(r.until());
                self.scopes.pop();
            }
            Stmt::If(i) => {
                self.expr(i.condition());
                self.block(i.block());
                for e in i.else_if().into_iter().flatten() {
                    self.expr(e.condition());
                    self.block(e.block());
                }
                if let Some(b) = i.else_block() {
                    self.block(b);
                }
            }
            Stmt::NumericFor(f) => {
                self.expr(f.start());
                self.expr(f.end());
                if let Some(s) = f.step() {
                    self.expr(s);
                }
                self.scopes.push(HashMap::new());
                self.declare(name_of(f.index_variable()), None);
                self.block(f.block());
                self.scopes.pop();
            }
            Stmt::GenericFor(f) => {
                for e in f.expressions().iter() {
                    self.expr(e);
                }
                self.scopes.push(HashMap::new());
                for n in f.names().iter() {
                    self.declare(name_of(n), None);
                }
                self.block(f.block());
                self.scopes.pop();
            }
            _ => {}
        }
    }

    /// Warns once per name about assigning a global.
    fn global_assignment(&mut self, name: String, at: (usize, usize), is_fn: bool) {
        if !GLOBAL_ALLOW.contains(&name.as_str()) && !self.assigned_globals.contains(&name) {
            let msg = if is_fn {
                format!(
                    "function '{name}' is a global; declare it local (local function {name} ...)"
                )
            } else {
                format!("variable '{name}' is a global; declare it local (local {name} = ...)")
            };
            self.diag(at, Severity::Warning, msg);
        }
        self.assigned_globals.insert(name);
    }

    fn assign_target(&mut self, v: &Var) {
        match v {
            Var::Name(t) => {
                let n = name_of(t);
                if self.lookup(&n).is_none() && !STD_GLOBALS.contains(&n.as_str()) {
                    self.global_assignment(n, tok_pos(t), false);
                }
            }
            Var::Expression(ve) => {
                let suffixes: Vec<&Suffix> = ve.suffixes().collect();
                if let Some((last, rest)) = suffixes.split_last() {
                    self.chain(ve.prefix(), rest);
                    if let Suffix::Index(Index::Brackets { expression, .. }) = last {
                        self.expr(expression);
                    }
                }
            }
            _ => {}
        }
    }

    fn body(&mut self, b: &FunctionBody, method: bool) {
        let planned = self.fn_aliases.remove(&body_key(b)).unwrap_or_default();
        self.scopes.push(HashMap::new());
        if method {
            self.declare("self".into(), None);
        }
        for (i, p) in b.parameters().iter().enumerate() {
            if let Parameter::Name(t) = p {
                let n = name_of(t);
                let planned = planned.iter().find(|(j, _)| *j == i).map(|(_, a)| *a);
                let alias = planned.or((n == "psi").then_some(Alias::Psi));
                self.declare(n, alias);
            }
        }
        self.block(b.block());
        self.scopes.pop();
    }

    /// Walks an expression; returns what it refers to in the API, if anything.
    fn expr(&mut self, e: &Expression) -> Option<Alias> {
        match e {
            Expression::BinaryOperator { lhs, binop, rhs } => {
                if let ast::BinOp::Plus(t) = binop {
                    let stringy = |e: &Expression| match strip_parens(e) {
                        Expression::String(s) => {
                            string_value(s).is_some_and(|v| !is_numeric_string(&v))
                        }
                        _ => false,
                    };
                    if stringy(lhs) || stringy(rhs) {
                        let msg = "+ on a string adds numbers; use .. to join strings".into();
                        self.diag(tok_pos(t), Severity::Warning, msg);
                    }
                }
                self.expr(lhs);
                self.expr(rhs);
                None
            }
            Expression::Parentheses { expression, .. } => {
                self.expr(expression);
                None
            }
            Expression::UnaryOperator { expression, .. } => {
                self.expr(expression);
                None
            }
            Expression::Function(f) => {
                self.body(f.body(), false);
                None
            }
            Expression::FunctionCall(c) => {
                let suffixes: Vec<&Suffix> = c.suffixes().collect();
                self.chain(c.prefix(), &suffixes)
            }
            Expression::TableConstructor(t) => {
                self.table(t);
                None
            }
            Expression::Var(Var::Name(t)) => self.read_name(t),
            Expression::Var(Var::Expression(ve)) => {
                let suffixes: Vec<&Suffix> = ve.suffixes().collect();
                self.chain(ve.prefix(), &suffixes)
            }
            _ => None,
        }
    }

    fn table(&mut self, t: &ast::TableConstructor) {
        for f in t.fields().iter() {
            match f {
                Field::ExpressionKey { key, value, .. } => {
                    self.expr(key);
                    self.expr(value);
                }
                Field::NameKey { value, .. } => {
                    self.expr(value);
                }
                Field::NoKey(v) => {
                    self.expr(v);
                }
                _ => {}
            }
        }
    }

    fn read_name(&mut self, t: &TokenReference) -> Option<Alias> {
        let n = name_of(t);
        if let Some(alias) = self.lookup(&n) {
            return alias;
        }
        if n == "psi" {
            return Some(Alias::Psi);
        }
        if let Some((_, msg)) = OLD_GLOBALS.iter().find(|(old, _)| *old == n) {
            self.diag(tok_pos(t), Severity::Warning, (*msg).into());
        } else if !STD_GLOBALS.contains(&n.as_str()) {
            self.global_reads.push((n, tok_pos(t)));
        }
        None
    }

    /// `prefix.a.b(args):c(...)`: resolves API members and checks calls.
    fn chain(&mut self, prefix: &Prefix, suffixes: &[&Suffix]) -> Option<Alias> {
        let (mut cur, mut at) = match prefix {
            Prefix::Name(t) => {
                let n = name_of(t);
                if n == "table"
                    && self.lookup("table").is_none()
                    && let Some(Suffix::Index(Index::Dot { name, .. })) = suffixes.first()
                    && name_of(name) == "getn"
                {
                    let msg = "table.getn is Lua 5.1; use #t".into();
                    self.diag(tok_pos(name), Severity::Warning, msg);
                }
                (self.read_name(t), tok_pos(t))
            }
            Prefix::Expression(e) => (self.expr(e), node_pos(&**e)),
            _ => (None, (0, 0)),
        };
        for s in suffixes {
            match s {
                Suffix::Index(Index::Dot { name, .. }) => {
                    at = tok_pos(name);
                    let Some(a) = cur else { continue };
                    let n = name_of(name);
                    cur = match member(a, &n) {
                        Ok(next) => next,
                        Err(cands) => {
                            let path = a.path();
                            let s = did_you_mean(suggest(&n, cands), &format!("{path}."));
                            let msg = format!("{path}.{n} is not part of the API{s}");
                            self.diag(at, Severity::Error, msg);
                            None
                        }
                    };
                }
                Suffix::Index(Index::Brackets { expression, .. }) => {
                    self.expr(expression);
                    cur = None;
                }
                Suffix::Call(Call::AnonymousCall(args)) => {
                    match cur {
                        Some(Alias::Func(f)) => self.call(f, args, at),
                        _ => self.args(args),
                    }
                    cur = None;
                }
                Suffix::Call(Call::MethodCall(mc)) => {
                    let n = name_of(mc.name());
                    if let Some(a) = cur
                        && !matches!(a, Alias::Func(_))
                    {
                        let path = a.path();
                        match member(a, &n) {
                            Ok(Some(Alias::Func(_))) => {
                                let msg = format!(
                                    "{path}:{n} passes {path} as the first argument; call {path}.{n}(...)"
                                );
                                self.diag(tok_pos(mc.name()), Severity::Error, msg);
                            }
                            Ok(_) => {}
                            Err(cands) => {
                                let s = did_you_mean(suggest(&n, cands), &format!("{path}."));
                                let msg = format!("{path}.{n} is not part of the API{s}");
                                self.diag(tok_pos(mc.name()), Severity::Error, msg);
                            }
                        }
                    }
                    self.args(mc.args());
                    cur = None;
                }
                _ => cur = None,
            }
        }
        cur
    }

    fn args(&mut self, args: &FunctionArgs) {
        match args {
            FunctionArgs::Parentheses { arguments, .. } => {
                for a in arguments.iter() {
                    self.expr(a);
                }
            }
            FunctionArgs::TableConstructor(t) => self.table(t),
            _ => {}
        }
    }

    /// A call to an API function: argument count, literal types, and the
    /// `ctx` parameters of the functions passed in.
    fn call(&mut self, f: &'static FnSig, args: &FunctionArgs, at: (usize, usize)) {
        let fname = format!("psi.{}", f.name);
        let list: Vec<&Expression> = match args {
            FunctionArgs::Parentheses { arguments, .. } => arguments.iter().collect(),
            FunctionArgs::String(_) | FunctionArgs::TableConstructor(_) => Vec::new(),
            _ => return,
        };
        // `f "x"` and `f {...}` pass one argument.
        let single = match args {
            FunctionArgs::String(t) => Some(Lit::Str(string_value(t))),
            FunctionArgs::TableConstructor(_) => Some(Lit::Table),
            _ => None,
        };
        let n = if single.is_some() { 1 } else { list.len() };
        // A call or `...` last can pass any number of values.
        let open = list.last().is_some_and(|e| match strip_parens(e) {
            Expression::FunctionCall(_) => !matches!(e, Expression::Parentheses { .. }),
            Expression::Symbol(t) => matches!(
                t.token_type(),
                TokenType::Symbol {
                    symbol: Symbol::Ellipsis
                }
            ),
            _ => false,
        });
        let max = f.params.len();
        let min = f.params.iter().filter(|p| !p.optional).count();
        let known = if open { n - 1 } else { n };
        if known > max || (!open && n < min) {
            let takes = match (min, max) {
                (_, 0) => "takes no arguments".to_owned(),
                (a, b) if a == b => format!("takes {b} {}", plural(b, "argument")),
                (a, b) => format!("takes {a} to {b} arguments"),
            };
            self.diag(at, Severity::Error, format!("{fname} {takes}, got {n}"));
        }
        if let (Some(lit), Some(p)) = (&single, f.params.first()) {
            self.check_literal(&fname, 1, p, lit, at);
        }
        for (i, (e, p)) in list.iter().zip(f.params).enumerate() {
            if open && i + 1 == list.len() {
                break;
            }
            let ty = parse_ty(p.ty);
            if let Some(lit) = literal(e) {
                self.check_literal(&fname, i + 1, p, &lit, node_pos(*e));
            }
            if let (Expression::TableConstructor(t), Ty::List(inner)) =
                (strip_parens(e), unwrap_optional(&ty))
            {
                self.check_list(&fname, i + 1, p, t, inner);
            }
            self.plan(e, &ty);
        }
        if let (FunctionArgs::TableConstructor(t), Some(p)) = (args, f.params.first()) {
            self.plan_table(t, &parse_ty(p.ty));
        }
        self.args(args);
    }

    fn check_literal(
        &mut self,
        fname: &str,
        i: usize,
        p: &psi_ext_api::ParamSig,
        lit: &Lit,
        at: (usize, usize),
    ) {
        let ty = parse_ty(p.ty);
        if accepts(&ty, lit) {
            return;
        }
        if let (Ty::Named(n), Lit::Str(Some(s))) = (unwrap_optional(&ty), lit)
            && is_event_name(n)
        {
            let sug = did_you_mean(suggest(s, EVENT_NAMES.iter().copied()), "");
            self.diag(
                at,
                Severity::Error,
                format!("{fname}: unknown event \"{s}\"{sug}"),
            );
            return;
        }
        let got = match lit {
            Lit::Str(Some(s)) if matches!(unwrap_optional(&ty), Ty::Literals(_)) => {
                format!("\"{s}\"")
            }
            l => l.type_name().to_owned(),
        };
        let expected = unwrap_optional(&ty).to_string();
        let msg = format!(
            "{fname}: argument {i} '{}' expected {expected}, got {got}",
            p.name
        );
        self.diag(at, Severity::Error, msg);
    }

    /// The literal elements of a `T[]` argument.
    fn check_list(
        &mut self,
        fname: &str,
        i: usize,
        p: &psi_ext_api::ParamSig,
        t: &ast::TableConstructor,
        inner: &Ty,
    ) {
        for (k, f) in t.fields().iter().enumerate() {
            if let Field::NoKey(v) = f
                && let Some(lit) = literal(v)
                && !accepts(inner, &lit)
            {
                let got = lit.type_name();
                let msg = format!(
                    "{fname}: argument {i} '{}' expected {inner}[], got {got} at [{}]",
                    p.name,
                    k + 1
                );
                self.diag(node_pos(v), Severity::Error, msg);
                return;
            }
        }
    }

    /// Marks the `Context` parameters of functions passed where the API
    /// declares a function type, and warns about unknown fields in table
    /// constructors passed as a named type.
    fn plan(&mut self, e: &Expression, ty: &Ty) {
        match (strip_parens(e), unwrap_optional(ty)) {
            (Expression::Function(f), Ty::Function { params, .. }) => {
                let ctx: Vec<(usize, Alias)> = params
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, t))| *t == Ty::Named("Context".into()))
                    .map(|(i, _)| (i, Alias::Ctx))
                    .collect();
                if !ctx.is_empty() {
                    self.fn_aliases.insert(body_key(f.body()), ctx);
                }
            }
            (Expression::TableConstructor(t), _) => self.plan_table(t, ty),
            _ => {}
        }
    }

    fn plan_table(&mut self, t: &ast::TableConstructor, ty: &Ty) {
        match unwrap_optional(ty) {
            Ty::Named(n) => {
                let Some(sig) = type_sig(n) else { return };
                if sig.fields.is_empty() {
                    return;
                }
                for f in t.fields().iter() {
                    let Field::NameKey { key, value, .. } = f else {
                        continue;
                    };
                    let k = name_of(key);
                    match sig.fields.iter().find(|f| f.name == k) {
                        Some(field) => self.plan(value, &parse_ty(field.ty)),
                        None => {
                            let s =
                                did_you_mean(suggest(&k, sig.fields.iter().map(|f| f.name)), "");
                            let msg = format!("{n} has no field '{k}'{s}");
                            self.diag(tok_pos(key), Severity::Warning, msg);
                        }
                    }
                }
            }
            Ty::Map(_, v) => {
                for f in t.fields().iter() {
                    if let Field::NameKey { value, .. } | Field::ExpressionKey { value, .. } = f {
                        self.plan(value, v);
                    }
                }
            }
            Ty::List(v) => {
                for f in t.fields().iter() {
                    if let Field::NoKey(value) = f {
                        self.plan(value, v);
                    }
                }
            }
            _ => {}
        }
    }
}

// ---- Dry load ---------------------------------------------------------------

/// How long loading (and then setup) may run.
const LOAD_TIME: Duration = Duration::from_secs(2);
const LOAD_MEMORY: usize = 64 << 20;
/// The stop error's text; the wrappers re-raise it.
const KILLED: &str = "psi ext check: stopped, ran too long";
const MANIFEST_KEYS: &[&str] = &["name", "version", "description", "setup"];

/// An [`ExtHost`] that records what an extension registers and answers
/// everything else harmlessly: ui calls get false or nil, `execute_tool` an
/// empty result, style functions the text unchanged. Registration specs are
/// checked like the real host does.
#[derive(Debug, Default)]
pub struct RecordingHost {
    /// "tool name", "command /name", "on event", ...
    pub registered: RefCell<Vec<String>>,
    /// Printed, logged and notified text, in order ("print: ...").
    pub output: RefCell<Vec<String>>,
}

fn rt_err(msg: String) -> mlua::Error {
    mlua::Error::runtime(msg)
}

fn lua_type(v: &Value) -> &'static str {
    match v {
        Value::Integer(_) => "number",
        v => v.type_name(),
    }
}

/// `t[key]`, which must have Lua type `want` (or be nil when `optional`).
fn field(t: &Table, what: &str, key: &str, want: &str, optional: bool) -> mlua::Result<Value> {
    let v: Value = t.raw_get(key)?;
    if lua_type(&v) == want || (optional && v.is_nil()) {
        return Ok(v);
    }
    let want = if optional {
        format!("{want}?")
    } else {
        want.to_owned()
    };
    Err(rt_err(format!(
        "{what}: field '{key}' expected {want}, got {}",
        lua_type(&v)
    )))
}

fn spec_table(what: &str, spec: Value) -> mlua::Result<Table> {
    match spec {
        Value::Table(t) => Ok(t),
        v => Err(rt_err(format!(
            "{what}: expected a table, got {}",
            lua_type(&v)
        ))),
    }
}

impl RecordingHost {
    fn record(&self, what: &str, entry: String) -> mlua::Result<()> {
        let mut r = self.registered.borrow_mut();
        if !entry.starts_with("on ") && !entry.starts_with("events.on ") && r.contains(&entry) {
            return Err(rt_err(format!("{what}: {entry} is registered twice")));
        }
        r.push(entry);
        Ok(())
    }

    fn out(&self, line: String) {
        self.output.borrow_mut().push(line);
    }

    /// A `{ description, handler }` spec (commands and shortcuts).
    fn handler_spec(what: &str, spec: Value) -> mlua::Result<()> {
        let t = spec_table(what, spec)?;
        field(&t, what, "description", "string", false)?;
        field(&t, what, "handler", "function", false)?;
        Ok(())
    }
}

impl ExtHost for RecordingHost {
    fn on(&self, _: &Lua, event: String, _: Function) -> mlua::Result<()> {
        self.record("psi.on", format!("on {event}"))
    }

    fn register_tool(&self, _: &Lua, spec: Value) -> mlua::Result<()> {
        let what = "psi.register_tool";
        let t = spec_table(what, spec)?;
        let name = field(&t, what, "name", "string", false)?;
        let name = name.to_string()?;
        if !valid_tool_name(&name) {
            let msg = format!(
                "{what}: name '{name}' must be lowercase letters, digits and _, starting with a letter"
            );
            return Err(rt_err(msg));
        }
        if field(&t, what, "description", "string", false)?
            .to_string()?
            .trim()
            .is_empty()
        {
            return Err(rt_err(format!("{what}: field 'description' is empty")));
        }
        let Value::Table(params) = field(&t, what, "params", "table", false)? else {
            return Err(rt_err(format!("{what}: field 'params' expected table")));
        };
        let mut names = Vec::new();
        for pair in params.pairs::<Value, Value>() {
            let (k, v) = pair?;
            let Value::String(k) = k else {
                return Err(rt_err(format!(
                    "{what}: params keys must be strings, got {}",
                    lua_type(&k)
                )));
            };
            let k = k.to_string_lossy();
            let pw = format!("{what}: param '{k}'");
            let p = spec_table(&pw, v)?;
            let ty = field(&p, &pw, "type", "string", false)?.to_string()?;
            if !PARAM_TYPES.contains(&ty.as_str()) {
                return Err(rt_err(format!(
                    "{pw}: type '{ty}' is not one of {}",
                    PARAM_TYPES.join(", ")
                )));
            }
            field(&p, &pw, "description", "string", false)?;
            names.push(k);
        }
        if let Value::Table(req) = field(&t, what, "required", "table", true)? {
            for v in req.sequence_values::<Value>() {
                match v? {
                    Value::String(s) if names.contains(&s.to_string_lossy()) => {}
                    Value::String(s) => {
                        let s = s.to_string_lossy();
                        return Err(rt_err(format!(
                            "{what}: required names '{s}', which is not in params"
                        )));
                    }
                    v => {
                        return Err(rt_err(format!(
                            "{what}: required expected string[], got {}",
                            lua_type(&v)
                        )));
                    }
                }
            }
        }
        if let Value::Table(a) = field(&t, what, "annotations", "table", true)? {
            for key in ["read_only", "destructive", "open_world"] {
                field(&a, &format!("{what}: annotations"), key, "boolean", true)?;
            }
        }
        field(&t, what, "always_ask", "boolean", true)?;
        field(&t, what, "execute", "function", false)?;
        self.record(what, format!("tool {name}"))
    }

    fn register_command(&self, _: &Lua, name: String, spec: Value) -> mlua::Result<()> {
        let what = "psi.register_command";
        let name = name.trim_start_matches('/');
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(rt_err(format!("{what}: name '{name}' must be one word")));
        }
        Self::handler_spec(what, spec)?;
        self.record(what, format!("command /{name}"))
    }

    fn register_shortcut(&self, _: &Lua, key: String, spec: Value) -> mlua::Result<()> {
        let what = "psi.register_shortcut";
        Self::handler_spec(what, spec)?;
        self.record(what, format!("shortcut {}", key.to_ascii_lowercase()))
    }

    fn register_provider(&self, _: &Lua, name: String, spec: Value) -> mlua::Result<()> {
        let what = "psi.register_provider";
        let t = spec_table(what, spec)?;
        field(&t, what, "base_url", "string", false)?;
        field(&t, what, "models", "table", false)?;
        self.record(what, format!("provider {name}"))
    }

    fn register_flag(&self, _: &Lua, name: String, spec: Value) -> mlua::Result<()> {
        let what = "psi.register_flag";
        let t = spec_table(what, spec)?;
        field(&t, what, "description", "string", false)?;
        self.record(what, format!("flag {name}"))
    }

    fn get_flag(&self, _: &Lua, _: String) -> mlua::Result<Value> {
        Ok(Value::Nil)
    }

    fn register_tool_renderer(&self, _: &Lua, tool: String, _: Function) -> mlua::Result<()> {
        self.record("psi.register_tool_renderer", format!("renderer {tool}"))
    }

    fn send_user_message(&self, _: &Lua, _: String, _: Option<Value>) -> mlua::Result<()> {
        Ok(())
    }

    fn send_message(&self, _: &Lua, _: String, _: String) -> mlua::Result<()> {
        Ok(())
    }

    fn append_entry(&self, _: &Lua, _: String, _: Value) -> mlua::Result<()> {
        Ok(())
    }

    fn entries(&self, lua: &Lua, _: String) -> mlua::Result<Value> {
        Ok(Value::Table(lua.create_table()?))
    }

    fn set_active_tools(&self, _: &Lua, _: Vec<String>) -> mlua::Result<()> {
        Ok(())
    }

    fn get_active_tools(&self, _: &Lua) -> mlua::Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn execute_tool(&self, lua: &Lua, _: String, _: Value) -> mlua::Result<Value> {
        let t = lua.create_table()?;
        t.set("text", "")?;
        t.set("is_error", false)?;
        Ok(Value::Table(t))
    }

    fn events_emit(&self, _: &Lua, _: String, _: Value) -> mlua::Result<()> {
        Ok(())
    }

    fn events_on(&self, _: &Lua, name: String, _: Function) -> mlua::Result<()> {
        self.record("psi.events.on", format!("events.on {name}"))
    }

    fn ui_notify(&self, _: &Lua, message: String, level: Option<String>) -> mlua::Result<()> {
        self.out(format!(
            "notify {}: {message}",
            level.as_deref().unwrap_or("info")
        ));
        Ok(())
    }

    async fn ui_confirm(&self, _: &Lua, _: String, _: String) -> mlua::Result<bool> {
        Ok(false)
    }

    async fn ui_select(&self, _: &Lua, _: String, _: Vec<String>) -> mlua::Result<Option<String>> {
        Ok(None)
    }

    async fn ui_input(
        &self,
        _: &Lua,
        _: String,
        _: Option<String>,
    ) -> mlua::Result<Option<String>> {
        Ok(None)
    }

    fn ui_status(&self, _: &Lua, _: String, _: Option<String>) -> mlua::Result<()> {
        Ok(())
    }

    fn ui_widget(&self, _: &Lua, _: String, _: Option<Vec<String>>) -> mlua::Result<()> {
        Ok(())
    }

    async fn ui_custom(&self, _: &Lua, component: Value) -> mlua::Result<Value> {
        let what = "psi.ui.custom";
        let t = spec_table(what, component)?;
        field(&t, what, "render", "function", false)?;
        field(&t, what, "handle_input", "function", false)?;
        Ok(Value::Nil)
    }

    fn style_color(&self, _: &Lua, _: String, text: String) -> mlua::Result<String> {
        Ok(text)
    }

    fn style_bold(&self, _: &Lua, text: String) -> mlua::Result<String> {
        Ok(text)
    }

    fn style_dim(&self, _: &Lua, text: String) -> mlua::Result<String> {
        Ok(text)
    }

    fn log(&self, _: &Lua, message: String) -> mlua::Result<()> {
        self.out(format!("log: {message}"));
        Ok(())
    }
}

/// Re-raises the stop error through pcall, xpcall and coroutine.resume (a
/// loop around them would go on otherwise), and sends print to `sink`.
/// In psi-ext, `limits::install` does the same for the real host.
const PRELUDE: &str = r#"
local KILLED, sink = ...
local function killed(e) return tostring(e):find(KILLED, 1, true) ~= nil end
local function check(ok, ...)
  if not ok and killed((...)) then error((...), 0) end
  return ok, ...
end
local raw_pcall, raw_xpcall, raw_resume = pcall, xpcall, coroutine.resume
pcall = function(f, ...) return check(raw_pcall(f, ...)) end
xpcall = function(f, h, ...)
  return check(raw_xpcall(f, function(e) if killed(e) then return e end return h(e) end, ...))
end
coroutine.resume = function(co, ...) return check(raw_resume(co, ...)) end
print = function(...)
  local t = table.pack(...)
  for i = 1, t.n do t[i] = tostring(t[i]) end
  sink(table.concat(t, "\t"))
end
"#;

/// Runs the file in a fresh state, checks the manifest it returns, and calls
/// `setup(psi)` against a [`RecordingHost`].
fn dry_load(main: &Path, src: &str, dir: Option<&Path>, info: &MainInfo, r: &mut Report) {
    let host = Rc::new(RecordingHost::default());
    let chunk = main
        .file_name()
        .map_or_else(|| "init.lua".into(), |n| n.to_string_lossy().into_owned());
    let deadline: Rc<Cell<Option<Instant>>> = Rc::default();
    let killed_at: Rc<Cell<Option<usize>>> = Rc::default();
    let result = (|| -> Result<(), (Option<usize>, String)> {
        let lua = Lua::new();
        let setup_err = |e: mlua::Error| (None, format!("could not set up the check: {e}"));
        lua.set_memory_limit(LOAD_MEMORY).map_err(setup_err)?;
        let (d, k, c) = (deadline.clone(), killed_at.clone(), chunk.clone());
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(1000),
            move |_, dbg| {
                if d.get().is_some_and(|t| Instant::now() > t) {
                    if k.get().is_none() && dbg.source().short_src.as_deref() == Some(c.as_str()) {
                        k.set(dbg.current_line());
                    }
                    return Err(mlua::Error::runtime(KILLED));
                }
                Ok(VmState::Continue)
            },
        )
        .map_err(setup_err)?;
        let h = host.clone();
        let sink = lua
            .create_function(move |_, s: String| {
                h.out(format!("print: {s}"));
                Ok(())
            })
            .map_err(setup_err)?;
        lua.load(PRELUDE)
            .set_name("=psi ext check")
            .call::<()>((KILLED, sink))
            .map_err(setup_err)?;
        if let Some(dir) = dir {
            let d = dir.display();
            let set = format!(
                "package.path = {:?} .. package.path",
                format!("{d}/?.lua;{d}/?/init.lua;")
            );
            lua.load(set).exec().map_err(setup_err)?;
        }
        let run = |f: &Function,
                   args: MultiValue,
                   phase: &str|
         -> Result<Value, (Option<usize>, String)> {
            deadline.set(Some(Instant::now() + LOAD_TIME));
            let out = f.call::<Value>(args);
            deadline.set(None);
            out.map_err(|e| describe(&e, &chunk, killed_at.get(), phase))
        };
        let f = lua
            .load(src)
            .set_name(format!("@{chunk}"))
            .into_function()
            .map_err(|e| describe(&e, &chunk, None, "loading"))?;
        let manifest = run(&f, MultiValue::new(), "loading")?;
        let Some(setup) = check_manifest(&manifest, main, info, r) else {
            return Ok(());
        };
        let psi = psi_ext_api::register(&lua, host.clone()).map_err(setup_err)?;
        run(
            &setup,
            MultiValue::from_vec(vec![Value::Table(psi)]),
            "setup",
        )?;
        Ok(())
    })();
    if let Err((line, msg)) = result {
        r.add(
            main,
            (line.unwrap_or(0), usize::from(line.is_some())),
            Severity::Error,
            msg,
        );
    }
    r.registered = host.registered.take();
    r.output = host.output.take();
}

/// The line (in `chunk`) and message of a Lua error, without the traceback.
fn describe(
    e: &mlua::Error,
    chunk: &str,
    killed_at: Option<usize>,
    phase: &str,
) -> (Option<usize>, String) {
    fn flatten(e: &mlua::Error, trace: &mut String) -> String {
        match e {
            mlua::Error::CallbackError { traceback, cause } => {
                let m = flatten(cause, trace);
                trace.push('\n');
                trace.push_str(traceback);
                m
            }
            mlua::Error::WithContext { cause, .. } => flatten(cause, trace),
            mlua::Error::RuntimeError(s) | mlua::Error::MemoryError(s) => {
                match s.split_once("\nstack traceback:") {
                    Some((m, t)) => {
                        trace.push_str(t);
                        m.to_owned()
                    }
                    None => s.clone(),
                }
            }
            mlua::Error::SyntaxError { message, .. } => message.clone(),
            mlua::Error::ExternalError(e) => e.to_string(),
            e => e.to_string(),
        }
    }
    let mut trace = String::new();
    let mut msg = flatten(e, &mut trace);
    let at = format!("{chunk}:");
    let line_after = |s: &str| -> Option<usize> {
        let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    let mut line = None;
    if let Some(rest) = msg.strip_prefix(&at)
        && let Some(n) = line_after(rest)
    {
        line = Some(n);
        msg = rest[n.to_string().len()..]
            .trim_start_matches(':')
            .trim_start()
            .to_owned();
    }
    if line.is_none() {
        line = trace
            .match_indices(&at)
            .find_map(|(i, _)| line_after(&trace[i + at.len()..]));
    }
    if msg.contains(KILLED) {
        let secs = LOAD_TIME.as_secs();
        return (
            killed_at.or(line),
            format!("{phase} ran for more than {secs}s and was stopped (an endless loop?)"),
        );
    }
    if matches!(e, mlua::Error::MemoryError(_)) {
        let mib = LOAD_MEMORY >> 20;
        return (
            line,
            format!("{phase} ran out of memory (the check allows {mib} MiB)"),
        );
    }
    (line, format!("{phase} failed: {msg}"))
}

/// Checks the returned manifest; returns `setup` when it is a function.
fn check_manifest(v: &Value, main: &Path, info: &MainInfo, r: &mut Report) -> Option<Function> {
    let ret = info.ret.unwrap_or((0, 0));
    let Value::Table(t) = v else {
        let msg = format!(
            "the file must return a manifest table {{ name, version, description, setup }}, got {}",
            lua_type(v)
        );
        r.add(main, ret, Severity::Error, msg);
        return None;
    };
    let at = |k: &str| info.fields.get(k).copied().unwrap_or(ret);
    let want = |k: &str| if k == "setup" { "function" } else { "string" };
    for key in MANIFEST_KEYS {
        let v: Value = t.raw_get(*key).unwrap_or(Value::Nil);
        let w = want(key);
        if v.is_nil() {
            // "Manifest" alone reads as a file: a model once spent 20 steps
            // writing manifest.json (2026-11-01, H-01).
            let msg = if *key == "setup" {
                "the table this file returns (the manifest) is missing 'setup', the function psi calls with the API: return { name = ..., version = ..., description = ..., setup = setup }".to_owned()
            } else {
                format!("the table this file returns (the manifest) is missing '{key}' (a {w})")
            };
            r.add(main, ret, Severity::Error, msg);
        } else if lua_type(&v) != w {
            let msg = format!("manifest field '{key}' expected {w}, got {}", lua_type(&v));
            r.add(main, at(key), Severity::Error, msg);
        } else if *key == "name"
            && let Value::String(s) = &v
            && !valid_ext_name(&s.to_string_lossy())
        {
            let msg = format!(
                "manifest name '{}' must be lowercase letters, digits, _ and -, starting with a letter",
                s.to_string_lossy()
            );
            r.add(main, at(key), Severity::Error, msg);
        }
    }
    for (k, _) in t.pairs::<Value, Value>().flatten() {
        let Value::String(k) = k else { continue };
        let k = k.to_string_lossy();
        if !MANIFEST_KEYS.contains(&k.as_str()) {
            let s = did_you_mean(suggest(&k, MANIFEST_KEYS.iter().copied()), "");
            r.add(
                main,
                at(&k),
                Severity::Warning,
                format!("unknown manifest field '{k}'{s}"),
            );
        }
    }
    t.raw_get::<Option<Function>>("setup").ok().flatten()
}

// ---- lua-language-server ----------------------------------------------------

const LUALS_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(serde::Deserialize)]
struct LualsDiag {
    #[serde(default)]
    code: String,
    message: String,
    range: LualsRange,
}

#[derive(serde::Deserialize)]
struct LualsRange {
    start: LualsPos,
}

#[derive(serde::Deserialize)]
struct LualsPos {
    line: usize,
    character: usize,
}

/// Runs `lua-language-server --check` on a copy of the files, with the API
/// stubs as a library and `---@param <p> psi` added above `setup` so the
/// handlers' `ev` and `ctx` are typed. Its findings become warnings.
fn luals(main: &Path, files: &[PathBuf], dir: Option<&Path>, info: &MainInfo, r: &mut Report) {
    let Some(bin) = find_on_path("lua-language-server") else {
        return;
    };
    let warn = |r: &mut Report, msg: String| {
        r.add(main, (0, 0), Severity::Warning, format!("luals: {msg}"))
    };
    let tmp = match tempfile::tempdir() {
        Ok(t) => t,
        Err(e) => return warn(r, format!("no temp dir: {e}")),
    };
    let (lib, ws, log) = (
        tmp.path().join("lib"),
        tmp.path().join("ws"),
        tmp.path().join("log"),
    );
    let root = dir.or_else(|| main.parent()).unwrap_or(Path::new("."));
    // Where the `---@param` line went (1-based, in the copy of main).
    let mut inserted = None;
    let mut copy = || -> std::io::Result<()> {
        std::fs::create_dir_all(&lib)?;
        std::fs::create_dir_all(&log)?;
        std::fs::write(lib.join("psi.lua"), LUALS_STUBS)?;
        for f in files {
            let rel = f.strip_prefix(root).unwrap_or(f);
            let dest = ws.join(rel);
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut src = std::fs::read_to_string(f)?;
            if f == main
                && let Some(SetupFn {
                    line,
                    param: Some(p),
                    ..
                }) = &info.setup
            {
                let lines: Vec<&str> = src.lines().collect();
                let typed = *line >= 2 && lines.get(line - 2).is_some_and(|l| l.contains("@param"));
                if !typed && *line >= 1 && *line <= lines.len() {
                    let indent: String = lines[line - 1]
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .collect();
                    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
                    out.insert(line - 1, format!("{indent}---@param {p} psi"));
                    src = out.join("\n") + "\n";
                    inserted = Some(*line);
                }
            }
            std::fs::write(dest, src)?;
        }
        let rc = serde_json::json!({
            "runtime.version": "Lua 5.4",
            "workspace.library": [lib.display().to_string()],
            "workspace.checkThirdParty": false,
            "diagnostics.globals": ["psi"],
        });
        std::fs::write(tmp.path().join("luarc.json"), rc.to_string())
    };
    if let Err(e) = copy() {
        return warn(r, format!("could not prepare the check: {e}"));
    }
    let child = Command::new(&bin)
        .arg(format!("--check={}", ws.display()))
        .arg("--check_format=json")
        .arg("--checklevel=Warning")
        .arg(format!("--logpath={}", log.display()))
        .arg(format!(
            "--configpath={}",
            tmp.path().join("luarc.json").display()
        ))
        .current_dir(tmp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return warn(r, format!("could not run {}: {e}", bin.display())),
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < LUALS_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return warn(
                    r,
                    format!(
                        "lua-language-server took over {}s; skipped",
                        LUALS_TIMEOUT.as_secs()
                    ),
                );
            }
        }
    }
    let results: HashMap<String, Vec<LualsDiag>> =
        match std::fs::read_to_string(log.join("check.json")) {
            // No findings is `[]`.
            Ok(t) if t.trim() == "[]" => return,
            Ok(t) => match serde_json::from_str(&t) {
                Ok(r) => r,
                Err(e) => return warn(r, format!("unreadable check.json: {e}")),
            },
            Err(_) => return warn(r, "lua-language-server wrote no results".into()),
        };
    let ws_real = ws.canonicalize().unwrap_or_else(|_| ws.clone());
    // A line the other checks flagged is about the same mistake; one luals
    // finding per line is enough too.
    let mut seen: HashSet<(PathBuf, usize)> =
        r.diags.iter().map(|d| (d.file.clone(), d.line)).collect();
    for (uri, diags) in results {
        let path = PathBuf::from(percent_decode(uri.strip_prefix("file://").unwrap_or(&uri)));
        let Some(rel) = path
            .strip_prefix(&ws)
            .or_else(|_| path.strip_prefix(&ws_real))
            .ok()
        else {
            continue;
        };
        let file = if dir.is_some() {
            root.join(rel)
        } else {
            main.to_owned()
        };
        for d in diags {
            let mut line = d.range.start.line + 1;
            if file == main
                && let Some(ins) = inserted
            {
                if line == ins {
                    continue;
                }
                if line > ins {
                    line -= 1;
                }
            }
            let col = d.range.start.character + 1;
            if !seen.insert((file.clone(), line)) {
                continue;
            }
            let code = if d.code.is_empty() {
                String::new()
            } else {
                format!(" ({})", d.code)
            };
            // Later lines repeat the first in detail; keep one line per finding.
            let first = d.message.lines().next().unwrap_or_default();
            let msg = format!("luals: {first}{code}");
            r.add(&file, (line, col), Severity::Warning, msg);
        }
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn run(name: &str) -> Report {
        check_with(&fixture(name), Options { luals: false })
    }

    /// The messages, as `line: severity: message`.
    fn msgs(r: &Report) -> Vec<String> {
        r.diags
            .iter()
            .map(|d| format!("{}: {}: {}", d.line, d.severity, d.message))
            .collect()
    }

    fn has(r: &Report, line: usize, sev: Severity, needle: &str) {
        assert!(
            r.diags
                .iter()
                .any(|d| d.line == line && d.severity == sev && d.message.contains(needle)),
            "no {sev} at line {line} containing {needle:?} in:\n{}",
            r.render()
        );
    }

    #[test]
    fn good_extension_single_file() {
        let r = run("good.lua");
        assert_eq!(r.diags, [], "{}", r.render());
        assert_eq!(
            r.registered,
            [
                "tool word_count",
                "command /todo",
                "shortcut ctrl+t",
                "flag verbose",
                "on tool_call",
                "on session_start",
                "events.on todo_changed",
                "renderer word_count",
            ]
        );
        assert_eq!(r.output, ["print: loaded good", "log: setup done"]);
        let text = r.render();
        assert!(text.ends_with("0 errors, 0 warnings; registered tool word_count, command /todo, shortcut ctrl+t, flag verbose, on tool_call, on session_start, events.on todo_changed, renderer word_count\n"), "{text}");
        assert!(text.contains("output: print: loaded good\n"), "{text}");
    }

    #[test]
    fn good_extension_directory_with_a_module() {
        let r = run("good_dir");
        assert_eq!(r.diags, [], "{}", r.render());
        assert_eq!(r.registered, ["tool lint_tool", "on tool_result"]);
    }

    #[test]
    fn api_calls_are_checked_against_the_signatures() {
        let r = run("api_mistakes.lua");
        let m = msgs(&r);
        has(
            &r,
            6,
            Severity::Error,
            "psi.ui.notfy is not part of the API; did you mean psi.ui.notify?",
        );
        has(
            &r,
            7,
            Severity::Error,
            "psi.ui.confirm takes 2 arguments, got 1",
        );
        has(
            &r,
            8,
            Severity::Error,
            "psi.ui.notify: argument 1 'message' expected string, got number",
        );
        has(
            &r,
            9,
            Severity::Error,
            "psi.ui.status: argument 1 'key' expected string, got table",
        );
        has(
            &r,
            10,
            Severity::Error,
            "psi.on: unknown event \"tool_cal\"; did you mean tool_call?",
        );
        has(
            &r,
            11,
            Severity::Error,
            "psi.ui.notify: argument 2 'level' expected \"info\"|\"warning\"|\"error\", got \"warn\"",
        );
        has(
            &r,
            12,
            Severity::Error,
            "psi.ui.notify takes 1 to 2 arguments, got 3",
        );
        // Through ctx, an alias and a method call.
        has(
            &r,
            14,
            Severity::Error,
            "psi.ui.selct is not part of the API; did you mean psi.ui.select?",
        );
        has(
            &r,
            15,
            Severity::Error,
            "psi.execute_tool takes 2 arguments, got 1",
        );
        has(
            &r,
            16,
            Severity::Error,
            "ctx.cdw is not part of the API; did you mean ctx.cwd?",
        );
        has(
            &r,
            19,
            Severity::Error,
            "psi.ui.notify: argument 1 'message' expected string, got nil",
        );
        has(
            &r,
            20,
            Severity::Error,
            "psi:log passes psi as the first argument; call psi.log(...)",
        );
        has(
            &r,
            21,
            Severity::Error,
            "psi.set_active_tools: argument 1 'names' expected string[], got number at [2]",
        );
        has(
            &r,
            22,
            Severity::Error,
            "psi.get_active_tools takes no arguments, got 1",
        );
        has(
            &r,
            23,
            Severity::Error,
            "psi.registr_tool is not part of the API; did you mean psi.register_tool?",
        );
        has(
            &r,
            26,
            Severity::Warning,
            "ToolSpec has no field 'descripton'; did you mean description?",
        );
        // The dry load stops at the first mistake.
        has(
            &r,
            6,
            Severity::Error,
            "setup failed: attempt to call a nil value (field 'notfy')",
        );
        // A call last may pass any number of values; correct calls pass.
        assert!(
            !m.iter()
                .any(|s| ["24:", "25:", "27:"].iter().any(|l| s.starts_with(l))),
            "{m:#?}"
        );
        assert_eq!(r.errors(), 16, "{}", r.render());
    }

    #[test]
    fn globals_and_pitfalls() {
        let r = run("globals.lua");
        has(
            &r,
            1,
            Severity::Warning,
            "variable 'counter' is a global; declare it local",
        );
        has(
            &r,
            2,
            Severity::Warning,
            "function 'helper' is a global; declare it local",
        );
        has(&r, 8, Severity::Warning, "unknown global 'undefined_thing'");
        has(
            &r,
            9,
            Severity::Warning,
            "unpack is Lua 5.1; use table.unpack",
        );
        has(&r, 10, Severity::Warning, "table.getn is Lua 5.1; use #t");
        has(&r, 11, Severity::Warning, "use .. to join strings");
        has(
            &r,
            12,
            Severity::Warning,
            "unknown global 'pirnt'; did you mean print?",
        );
        // `counter` and `helper` are read later without "unknown global";
        // locals, std globals, `"1" + 1` and upvalues are fine.
        assert_eq!(r.warnings(), 7, "{}", r.render());
        // The dry load trips over unpack.
        has(
            &r,
            9,
            Severity::Error,
            "loading failed: attempt to call a nil value (global 'unpack')",
        );
        assert_eq!(r.errors(), 1, "{}", r.render());
    }

    #[test]
    fn syntax_error_has_the_line() {
        let r = run("syntax.lua");
        assert_eq!(r.diags.len(), 1, "{}", r.render());
        has(&r, 4, Severity::Error, "syntax error");
        has(&r, 4, Severity::Error, "~=");
        assert!(!r.diags[0].message.contains("4:10"), "{}", r.render());
        assert!(r.registered.is_empty());
    }

    #[test]
    fn manifest_fields() {
        let r = run("manifest.lua");
        has(
            &r,
            1,
            Severity::Error,
            "the table this file returns (the manifest) is missing 'version' (a string)",
        );
        has(
            &r,
            2,
            Severity::Error,
            "manifest name 'Bad Name' must be lowercase",
        );
        has(
            &r,
            3,
            Severity::Error,
            "manifest field 'description' expected string, got number",
        );
        has(
            &r,
            4,
            Severity::Warning,
            "unknown manifest field 'setpu'; did you mean setup?",
        );
        has(
            &r,
            1,
            Severity::Error,
            "the table this file returns (the manifest) is missing 'setup', the function psi calls with the API: return { name = ..., version = ..., description = ..., setup = setup }",
        );
        assert_eq!(r.errors(), 4, "{}", r.render());
    }

    #[test]
    fn a_file_that_returns_no_table() {
        let r = run("no_manifest.lua");
        has(&r, 2, Severity::Error, "must return a manifest table");
    }

    #[test]
    fn setup_errors_have_the_message_and_line() {
        let r = run("setup_throws.lua");
        has(&r, 7, Severity::Error, "setup failed: boom");
        // What it registered first and printed are still reported.
        assert_eq!(r.registered, ["on input"]);
        assert_eq!(r.output, ["print: before"]);

        let r = run("bad_tool.lua");
        has(
            &r,
            6,
            Severity::Error,
            "setup failed: psi.register_tool: field 'execute' expected function, got nil",
        );

        let r = run("glue_error.lua");
        has(
            &r,
            6,
            Severity::Error,
            "setup failed: psi.ui.status: argument 1 'key' expected string, got table",
        );
    }

    #[test]
    fn a_runaway_loop_at_load_time_is_stopped() {
        let start = Instant::now();
        let r = run("runaway.lua");
        assert!(start.elapsed() < LOAD_TIME * 3, "{:?}", start.elapsed());
        has(
            &r,
            3,
            Severity::Error,
            "loading ran for more than 2s and was stopped",
        );
        assert_eq!(r.diags.len(), 1, "{}", r.render());
        // pcall doesn't swallow the stop.
        let r = run("runaway_setup.lua");
        has(
            &r,
            6,
            Severity::Error,
            "setup ran for more than 2s and was stopped",
        );
    }

    #[test]
    fn shape() {
        let r = run("not_an_extension");
        assert_eq!(r.errors(), 1);
        assert!(
            r.diags[0]
                .message
                .contains("a name.lua file, a directory with init.lua"),
            "{}",
            r.render()
        );
        let r = run("missing.lua");
        assert!(
            r.diags[0].message.starts_with("does not exist"),
            "{}",
            r.render()
        );
    }

    fn script(json: &str, mode: u32) -> (tempfile::TempDir, Report) {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("tool.json"), json).unwrap();
        let run = d.path().join("run.sh");
        std::fs::write(&run, "#!/bin/sh\ncat\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let r = check_with(d.path(), Options { luals: false });
        (d, r)
    }

    const GOOD_TOOL: &str = r#"{
  "name": "word_count",
  "description": "Count the words in a file",
  "params": {"path": {"type": "string", "description": "The file"}},
  "required": ["path"],
  "read_only": true,
  "command": "run.sh"
}"#;

    #[test]
    fn script_tool_good() {
        let (_d, r) = script(GOOD_TOOL, 0o755);
        assert_eq!(r.diags, [], "{}", r.render());
        assert_eq!(r.registered, ["tool word_count"]);
        let argv = GOOD_TOOL.replace(r#""run.sh""#, r#"["sh", "run.sh"]"#);
        let (_d, r) = script(&argv, 0o644);
        assert_eq!(r.diags, [], "{}", r.render());
    }

    #[test]
    fn script_tool_mistakes() {
        let bad = r#"{
  "name": "Word-Count",
  "params": {"path": {"type": "strin", "description": "The file"}},
  "required": ["file"],
  "read_ony": true,
  "command": "run.sh"
}"#;
        let (_d, r) = script(bad, 0o644);
        has(
            &r,
            2,
            Severity::Error,
            "name 'Word-Count' must be lowercase letters, digits and _",
        );
        has(&r, 1, Severity::Error, "missing 'description'");
        has(
            &r,
            3,
            Severity::Error,
            "params.path: type 'strin' is not one of string, integer",
        );
        has(
            &r,
            4,
            Severity::Error,
            "required names 'file', which is not in params",
        );
        has(
            &r,
            5,
            Severity::Warning,
            "unknown key 'read_ony'; did you mean read_only?",
        );
        has(
            &r,
            6,
            Severity::Error,
            "command 'run.sh' is not executable (chmod +x run.sh)",
        );
        assert_eq!(r.errors(), 5, "{}", r.render());

        let (_d, r) = script(&GOOD_TOOL.replace("run.sh", "nope.sh"), 0o755);
        has(
            &r,
            7,
            Severity::Error,
            "command 'nope.sh' is not in the tool directory or on PATH",
        );
        let (_d, r) = script(
            &GOOD_TOOL.replace(r#""Count the words in a file""#, r#""""#),
            0o755,
        );
        has(&r, 3, Severity::Error, "description is empty");
        let (_d, r) = script("{\"name\": ", 0o755);
        has(&r, 1, Severity::Error, "not valid JSON");
    }

    #[test]
    fn suggestions() {
        assert_eq!(suggest("notfy", ["notify", "confirm"]), Some("notify"));
        assert_eq!(suggest("zzzzzz", ["notify", "confirm"]), None);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn luals_warns_about_a_bad_payload_field() {
        if find_on_path("lua-language-server").is_none() {
            eprintln!("skipped: lua-language-server is not on PATH");
            return;
        }
        // The stubs give a correct extension no findings.
        let r = check(&fixture("good.lua"));
        assert_eq!(r.diags, [], "{}", r.render());
        let r = check(&fixture("luals_payload.lua"));
        has(&r, 7, Severity::Warning, "luals: Undefined field `nme`.");
        // The static pass's own finding isn't repeated by luals.
        has(
            &r,
            9,
            Severity::Error,
            "psi.ui.notfy is not part of the API",
        );
        let dup = r
            .diags
            .iter()
            .any(|d| d.message.starts_with("luals:") && d.message.contains("notfy"));
        assert!(!dup, "{}", r.render());
    }
}
