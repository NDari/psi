//! Generates the extension API from `api/*.toml` (psi-plan.md §9.5, D-14):
//! the `ExtHost` trait, the mlua glue and the signature tables (`ext_api.rs`),
//! the LuaLS stubs (`psi.lua`) and the markdown reference (`api.md`), all in
//! OUT_DIR. A bad definition fails the build with a message naming the item.

#[path = "src/types.rs"]
#[allow(dead_code)] // Only part of the grammar's API is needed here.
mod types;

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

use serde::Deserialize;
use types::Ty;

const API_DIR: &str = "api";

/// What each event kind means; build.rs checks every event uses one.
const KINDS: &[(&str, &str)] = &[
    (
        "notify",
        "Handlers are told; their return values are ignored.",
    ),
    (
        "chain",
        "Handlers run in turn; each may return a new value, which the next handler sees.",
    ),
    (
        "gate",
        "Any handler can block it; a handler that fails blocks it too.",
    ),
];

/// Names that would break the generated Rust or Lua if used for a parameter.
const RESERVED: &[&str] = &[
    "lua", "self", "Self", "type", "fn", "let", "mut", "ref", "move", "match", "loop", "impl",
    "use", "mod", "pub", "crate", "super", "struct", "enum", "trait", "where", "async", "await",
    "dyn", "const", "static", "return", "if", "else", "for", "while", "in", "break", "continue",
    "true", "false", "and", "or", "not", "nil", "end", "then", "do", "repeat", "until", "local",
    "function", "goto", "elseif",
];

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Api {
    #[serde(default, rename = "function")]
    functions: Vec<FunctionDef>,
    #[serde(default, rename = "event")]
    events: Vec<EventDef>,
    #[serde(default, rename = "type")]
    types: Vec<TypeDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionDef {
    name: String,
    #[serde(default)]
    doc: String,
    #[serde(default, rename = "async")]
    is_async: bool,
    #[serde(default)]
    params: Vec<ParamDef>,
    returns: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParamDef {
    name: String,
    #[serde(rename = "type")]
    ty: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventDef {
    name: String,
    kind: String,
    #[serde(default)]
    doc: String,
    payload: String,
    result: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeDef {
    name: String,
    #[serde(default)]
    doc: String,
    #[serde(default)]
    fields: Vec<FieldDef>,
    alias: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldDef {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    doc: String,
}

/// A function with its types parsed.
struct Func<'a> {
    def: &'a FunctionDef,
    params: Vec<(&'a str, Ty)>,
    ret: Option<Ty>,
    /// The subtable (`ui` for `ui.confirm`), if any.
    table: Option<&'a str>,
    /// The key in its table (`confirm`).
    key: &'a str,
    /// The `ExtHost` method (`ui_confirm`).
    method: String,
    /// Required params come first; this many.
    required: usize,
}

struct Event<'a> {
    def: &'a EventDef,
    payload: Ty,
    result: Option<Ty>,
}

struct Type<'a> {
    def: &'a TypeDef,
    fields: Vec<(&'a FieldDef, Ty)>,
}

struct Model<'a> {
    funcs: Vec<Func<'a>>,
    events: Vec<Event<'a>>,
    types: Vec<Type<'a>>,
    /// The `alias = "events"` type (EventName), if any.
    events_alias: Option<&'a str>,
}

impl Model<'_> {
    fn is_events(&self, t: &Ty) -> bool {
        matches!(t, Ty::Named(n) if Some(n.as_str()) == self.events_alias)
    }

    /// Subtables in order of first use.
    fn tables(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for t in self.funcs.iter().filter_map(|f| f.table) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
        out
    }
}

fn main() {
    println!("cargo:rerun-if-changed={API_DIR}");
    let api = load(Path::new(API_DIR));
    let model = validate(&api);
    let out = env::var("OUT_DIR").unwrap_or_else(|e| panic!("OUT_DIR: {e}"));
    let out = Path::new(&out);
    write(&out.join("ext_api.rs"), &gen_rust(&model));
    write(&out.join("psi.lua"), &gen_lua(&model));
    write(&out.join("api.md"), &gen_markdown(&model));
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

/// Reads every `*.toml` in the api directory, in name order, into one definition.
fn load(dir: &Path) -> Api {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    let mut api = Api::default();
    for path in files {
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let part: Api = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("{}: invalid API definition: {e}", path.display()));
        api.functions.extend(part.functions);
        api.events.extend(part.events);
        api.types.extend(part.types);
    }
    api
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn unique<'a>(seen: &mut HashSet<&'a str>, what: &str, name: &'a str) {
    if !seen.insert(name) {
        panic!("api: duplicate {what} `{name}`");
    }
}

/// Parses every type and checks names; panics (a build error) on the first problem.
fn validate(api: &Api) -> Model<'_> {
    let mut type_names = HashSet::new();
    let mut events_alias = None;
    for t in &api.types {
        if !is_ident(&t.name) || types::parse(&t.name) != Ok(Ty::Named(t.name.clone())) {
            panic!("api: type `{}`: not a valid type name", t.name);
        }
        unique(&mut type_names, "type", &t.name);
        match t.alias.as_deref() {
            None => {}
            Some("events") if t.fields.is_empty() && events_alias.is_none() => {
                events_alias = Some(t.name.as_str());
            }
            Some("events") => panic!(
                "api: type `{}`: an `alias = \"events\"` type has no fields, and there is only one",
                t.name
            ),
            Some(other) => panic!(
                "api: type `{}`: unknown alias `{other}` (only \"events\" is known)",
                t.name
            ),
        }
    }
    let ty = |s: &str, at: &dyn Fn() -> String| -> Ty {
        let t = types::parse(s).unwrap_or_else(|e| panic!("api: {}: bad type: {e}", at()));
        t.for_each_named(&mut |n| {
            if !type_names.contains(n) {
                panic!("api: {}: unknown type `{n}` in {s:?}", at());
            }
        });
        t
    };

    let mut fn_names = HashSet::new();
    let mut methods = HashSet::new();
    let mut funcs = Vec::new();
    for f in &api.functions {
        let (table, key) = match f.name.split_once('.') {
            Some((t, k)) => (Some(t), k),
            None => (None, f.name.as_str()),
        };
        if !table.is_none_or(is_ident) || !is_ident(key) {
            panic!(
                "api: function `{}`: use `name` or `table.name` with identifiers",
                f.name
            );
        }
        unique(&mut fn_names, "function", &f.name);
        let method = f.name.replace('.', "_");
        if !methods.insert(method.clone()) {
            panic!(
                "api: function `{}`: its ExtHost method `{method}` clashes with another function",
                f.name
            );
        }
        let mut param_names = HashSet::new();
        let mut params = Vec::new();
        let mut required = 0;
        for p in &f.params {
            let at = || format!("function `{}` param `{}`", f.name, p.name);
            if !is_ident(&p.name) || RESERVED.contains(&p.name.as_str()) {
                panic!("api: {}: not a usable parameter name", at());
            }
            unique(&mut param_names, &format!("param in `{}`", f.name), &p.name);
            let t = ty(&p.ty, &at);
            if !matches!(t, Ty::Optional(_)) {
                if required < params.len() {
                    panic!(
                        "api: {}: a required param can't follow an optional one",
                        at()
                    );
                }
                required += 1;
            }
            params.push((p.name.as_str(), t));
        }
        let ret = f
            .returns
            .as_deref()
            .map(|r| ty(r, &|| format!("function `{}` returns", f.name)));
        funcs.push(Func {
            def: f,
            params,
            ret,
            table,
            key,
            method,
            required,
        });
    }
    for f in &funcs {
        if f.table.is_none() && funcs.iter().any(|g| g.table == Some(f.key)) {
            panic!(
                "api: function `{}` has the name of a subtable (`{}.*`)",
                f.def.name, f.key
            );
        }
    }

    let mut event_names = HashSet::new();
    let mut events = Vec::new();
    for e in &api.events {
        let at = || format!("event `{}`", e.name);
        if !is_ident(&e.name) {
            panic!("api: {}: not a valid event name", at());
        }
        unique(&mut event_names, "event", &e.name);
        if !KINDS.iter().any(|(k, _)| *k == e.kind) {
            panic!(
                "api: {}: unknown kind `{}` (notify, chain or gate)",
                at(),
                e.kind
            );
        }
        if e.kind == "notify" && e.result.is_some() {
            panic!("api: {}: a notify event has no result", at());
        }
        events.push(Event {
            def: e,
            payload: ty(&e.payload, &|| format!("{} payload", at())),
            result: e
                .result
                .as_deref()
                .map(|r| ty(r, &|| format!("{} result", at()))),
        });
    }

    let mut types = Vec::new();
    for t in &api.types {
        let mut field_names = HashSet::new();
        let fields = t
            .fields
            .iter()
            .map(|f| {
                unique(&mut field_names, &format!("field in `{}`", t.name), &f.name);
                (
                    f,
                    ty(&f.ty, &|| format!("type `{}` field `{}`", t.name, f.name)),
                )
            })
            .collect();
        types.push(Type { def: t, fields });
    }

    Model {
        funcs,
        events,
        types,
        events_alias,
    }
}

// ---- Rust ------------------------------------------------------------------

fn rust_ty(m: &Model, t: &Ty) -> String {
    match t {
        Ty::String | Ty::Literals(_) => "String".into(),
        Ty::Integer => "i64".into(),
        Ty::Number => "f64".into(),
        Ty::Boolean => "bool".into(),
        Ty::Function { .. } => "mlua::Function".into(),
        Ty::List(t) if **t == Ty::String => "Vec<String>".into(),
        Ty::Optional(t) => format!("Option<{}>", rust_ty(m, t)),
        t if m.is_events(t) => "String".into(),
        _ => "mlua::Value".into(),
    }
}

fn rust_ret(m: &Model, t: Option<&Ty>) -> String {
    match t {
        None | Some(Ty::Nil) => "()".into(),
        Some(t) => rust_ty(m, t),
    }
}

/// How an argument's type reads in an error (`expected ...`).
fn expected(t: &Ty) -> String {
    match t {
        Ty::Function { .. } => "function".into(),
        t => t.to_string(),
    }
}

/// A Rust expression checking and converting the Value in `v`, as `mlua::Result<T>`.
fn conv(m: &Model, t: &Ty, arg: &str, v: &str) -> String {
    match t {
        Ty::String => format!("glue::string({arg}, {v})"),
        Ty::Integer => format!("glue::integer({arg}, {v})"),
        Ty::Number => format!("glue::number({arg}, {v})"),
        Ty::Boolean => format!("glue::boolean({arg}, {v})"),
        Ty::Function { .. } => format!("glue::function({arg}, {v})"),
        Ty::Literals(ls) => format!("glue::literal({arg}, {v}, &{ls:?})"),
        Ty::List(t) if **t == Ty::String => format!("glue::string_list({arg}, {v})"),
        Ty::Optional(t) => format!("glue::optional({v}, |v| {})", conv(m, t, arg, "v")),
        t if m.is_events(t) => format!("glue::literal({arg}, {v}, EVENT_NAMES)"),
        t => format!(
            "glue::value({arg}, {v}, {}, {:?})",
            shallow_closure(m, t),
            expected(t)
        ),
    }
}

/// A `Fn(&Value) -> bool` closure checking a value's type shallowly.
fn shallow_closure(m: &Model, t: &Ty) -> String {
    match t {
        Ty::Any => "|_| true".into(),
        t => format!("|v| {}", shallow(m, t)),
    }
}

fn shallow(m: &Model, t: &Ty) -> String {
    match t {
        Ty::Any => "true".into(),
        Ty::Nil => "v.is_nil()".into(),
        Ty::String => "v.is_string()".into(),
        Ty::Integer => "v.is_integer()".into(),
        Ty::Number => "(v.is_integer() || v.is_number())".into(),
        Ty::Boolean => "v.is_boolean()".into(),
        Ty::Function { .. } => "v.is_function()".into(),
        Ty::Literals(ls) => literal_check(ls),
        t if m.is_events(t) => {
            "v.as_string().is_some_and(|s| EVENT_NAMES.contains(&&*s.to_string_lossy()))".into()
        }
        Ty::Table | Ty::Named(_) | Ty::List(_) | Ty::Map(..) => "v.is_table()".into(),
        Ty::Optional(t) => format!("(v.is_nil() || {})", shallow(m, t)),
        Ty::Union(ts) => {
            let parts: Vec<String> = ts.iter().map(|t| shallow(m, t)).collect();
            format!("({})", parts.join(" || "))
        }
    }
}

fn literal_check(ls: &[String]) -> String {
    format!("v.as_string().is_some_and(|s| {ls:?}.contains(&&*s.to_string_lossy()))")
}

/// `psi.ui.confirm(title: string, message: string): boolean`
fn signature(f: &Func) -> String {
    let params: Vec<String> = f
        .def
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty))
        .collect();
    let ret = f
        .def
        .returns
        .as_ref()
        .map(|r| format!(": {r}"))
        .unwrap_or_default();
    format!("psi.{}({}){ret}", f.def.name, params.join(", "))
}

fn gen_rust(m: &Model) -> String {
    let mut o = String::new();
    let w = &mut o;
    w.push_str("// Generated by build.rs from api/*.toml. Do not edit.\n\n");

    // The trait.
    w.push_str(
        "/// The host side of the Lua API: one method per function in `api/*.toml`.\n\
         ///\n\
         /// The glue (`register`) has already checked and converted the arguments.\n\
         /// Implementations are used generically (never as `dyn`), so async methods\n\
         /// may be written as `async fn`.\n\
         pub trait ExtHost {\n\
         \x20   /// The capability check, called by the glue before every function with its\n\
         \x20   /// API name (like `\"ui.confirm\"`). Trusted extensions allow everything; the\n\
         \x20   /// sandboxed tier for public readiness (psi-plan.md §13) refuses here, and the\n\
         \x20   /// error reaches the calling Lua code.\n\
         \x20   fn check(&self, function: &'static str) -> mlua::Result<()> {\n\
         \x20       let _ = function;\n\
         \x20       Ok(())\n\
         \x20   }\n",
    );
    for f in &m.funcs {
        let tag = if f.def.is_async { " (async)" } else { "" };
        let _ = writeln!(w, "\n    #[doc = {:?}]", format!("`{}`{tag}", signature(f)));
        w.push_str("    ///\n");
        let _ = writeln!(w, "    #[doc = {:?}]", f.def.doc);
        let mut params = String::from("&self, lua: &mlua::Lua");
        for (name, t) in &f.params {
            let _ = write!(params, ", {name}: {}", rust_ty(m, t));
        }
        let ret = rust_ret(m, f.ret.as_ref());
        if f.def.is_async {
            let _ = writeln!(
                w,
                "    fn {}({params}) -> impl Future<Output = mlua::Result<{ret}>>;",
                f.method
            );
        } else {
            let _ = writeln!(w, "    fn {}({params}) -> mlua::Result<{ret}>;", f.method);
        }
    }
    w.push_str("}\n\n");

    if m.events_alias.is_some() {
        let names: Vec<&str> = m.events.iter().map(|e| e.def.name.as_str()).collect();
        let _ = writeln!(
            w,
            "/// Every event name.\npub static EVENT_NAMES: &[&str] = &{names:?};\n"
        );
    }

    // The glue.
    w.push_str(
        "/// Builds the `psi` table (with its subtables) for one Lua state, every\n\
         /// function checking its arguments and then calling `host`.\n\
         pub fn register<H: ExtHost + 'static>(\n\
         \x20   lua: &mlua::Lua,\n\
         \x20   host: std::rc::Rc<H>,\n\
         ) -> mlua::Result<mlua::Table> {\n\
         \x20   let psi = lua.create_table()?;\n",
    );
    for t in m.tables() {
        let _ = writeln!(w, "    let t_{t} = lua.create_table()?;");
        let _ = writeln!(w, "    psi.set({t:?}, &t_{t})?;");
    }
    for f in &m.funcs {
        let lua_name = format!("psi.{}", f.def.name);
        let table = f.table.map_or("psi".to_owned(), |t| format!("t_{t}"));
        let create = if f.def.is_async {
            "create_async_function"
        } else {
            "create_function"
        };
        let mut body = String::new();
        let b = &mut body;
        let _ = writeln!(b, "host.check({:?})?;", f.def.name);
        let _ = writeln!(
            b,
            "glue::arity({lua_name:?}, args.len(), {}, {})?;",
            f.required,
            f.params.len()
        );
        if !f.params.is_empty() {
            b.push_str("let mut args = args.into_iter();\n");
        }
        let mut call_args = Vec::new();
        for (i, (name, t)) in f.params.iter().enumerate() {
            let arg = format!(
                "&glue::Arg {{ func: {lua_name:?}, index: {}, name: {name:?} }}",
                i + 1
            );
            let _ = writeln!(
                b,
                "let p{} = {}?;",
                i + 1,
                conv(m, t, &arg, "args.next().unwrap_or_default()")
            );
            call_args.push(format!("p{}", i + 1));
        }
        let call_args: String = call_args.iter().map(|a| format!(", {a}")).collect();
        let _ = writeln!(w, "    {{\n        let host = std::rc::Rc::clone(&host);");
        if f.def.is_async {
            let _ = writeln!(b, "host.{}(&lua{call_args}).await", f.method);
            let _ = writeln!(
                w,
                "        let func = lua.{create}(move |lua, args: mlua::MultiValue| {{\n\
                 \x20           let host = std::rc::Rc::clone(&host);\n\
                 \x20           async move {{"
            );
            indent(w, &body, 16);
            w.push_str("            }\n        })?;\n");
        } else {
            let _ = writeln!(b, "host.{}(lua{call_args})", f.method);
            let _ = writeln!(
                w,
                "        let func = lua.{create}(move |lua, args: mlua::MultiValue| {{"
            );
            indent(w, &body, 12);
            w.push_str("        })?;\n");
        }
        let _ = writeln!(w, "        {table}.set({:?}, func)?;\n    }}", f.key);
    }
    w.push_str("    Ok(psi)\n}\n\n");

    // The signature tables.
    w.push_str(
        "/// Every API function, for `psi ext check`.\npub static FUNCTIONS: &[FnSig] = &[\n",
    );
    for f in &m.funcs {
        let params: Vec<String> = f
            .def
            .params
            .iter()
            .zip(&f.params)
            .map(|(p, (_, t))| {
                format!(
                    "ParamSig {{ name: {:?}, ty: {:?}, optional: {} }}",
                    p.name,
                    p.ty,
                    matches!(t, Ty::Optional(_))
                )
            })
            .collect();
        let _ = writeln!(
            w,
            "    FnSig {{ name: {:?}, doc: {:?}, params: &[{}], returns: {:?}, is_async: {} }},",
            f.def.name,
            f.def.doc,
            params.join(", "),
            f.def.returns,
            f.def.is_async
        );
    }
    w.push_str("];\n\n/// Every event.\npub static EVENTS: &[EventSig] = &[\n");
    for e in &m.events {
        let d = e.def;
        let _ = writeln!(
            w,
            "    EventSig {{ name: {:?}, kind: {:?}, doc: {:?}, payload: {:?}, result: {:?} }},",
            d.name, d.kind, d.doc, d.payload, d.result
        );
    }
    w.push_str("];\n\n/// Every named type.\npub static TYPES: &[TypeSig] = &[\n");
    for t in &m.types {
        let d = t.def;
        let fields: Vec<String> = d
            .fields
            .iter()
            .map(|f| {
                format!(
                    "FieldSig {{ name: {:?}, ty: {:?}, doc: {:?} }}",
                    f.name, f.ty, f.doc
                )
            })
            .collect();
        let _ = writeln!(
            w,
            "    TypeSig {{ name: {:?}, doc: {:?}, fields: &[{}], alias: {:?} }},",
            d.name,
            d.doc,
            fields.join(", "),
            d.alias
        );
    }
    w.push_str("];\n");
    o
}

fn indent(w: &mut String, text: &str, n: usize) {
    for line in text.lines() {
        let _ = writeln!(w, "{:n$}{line}", "");
    }
}

// ---- LuaLS stubs -----------------------------------------------------------

/// A type in LuaLS syntax.
fn lua_ty(t: &Ty) -> String {
    match t {
        Ty::String => "string".into(),
        Ty::Integer => "integer".into(),
        Ty::Number => "number".into(),
        Ty::Boolean => "boolean".into(),
        Ty::Table => "table".into(),
        Ty::Any => "any".into(),
        Ty::Nil => "nil".into(),
        Ty::Optional(t) => match **t {
            Ty::Union(_) | Ty::Literals(_) | Ty::Function { .. } => {
                format!("{}|nil", lua_atom(t))
            }
            _ => format!("{}?", lua_ty(t)),
        },
        Ty::List(t) => format!("{}[]", lua_atom(t)),
        Ty::Map(k, v) => format!("table<{}, {}>", lua_ty(k), lua_ty(v)),
        Ty::Literals(ls) => {
            let ls: Vec<String> = ls.iter().map(|l| format!("\"{l}\"")).collect();
            ls.join("|")
        }
        Ty::Union(ts) => {
            let ts: Vec<String> = ts.iter().map(lua_atom).collect();
            ts.join("|")
        }
        Ty::Function { params, ret } => {
            let ps: Vec<String> = params
                .iter()
                .map(|(n, t)| format!("{n}: {}", lua_ty(t)))
                .collect();
            let ret = ret
                .as_ref()
                .map(|r| format!(": {}", lua_ty(r)))
                .unwrap_or_default();
            format!("fun({}){ret}", ps.join(", "))
        }
        Ty::Named(n) => n.clone(),
    }
}

/// `lua_ty`, in parentheses where a postfix or `|` would bind wrongly.
fn lua_atom(t: &Ty) -> String {
    match t {
        Ty::Union(_) | Ty::Function { .. } | Ty::Optional(_) => format!("({})", lua_ty(t)),
        Ty::Literals(ls) if ls.len() > 1 => format!("({})", lua_ty(t)),
        t => lua_ty(t),
    }
}

/// `name?` and the inner type for an optional, else the name and type.
fn lua_name_ty(name: &str, t: &Ty) -> (String, String) {
    match t {
        Ty::Optional(inner) => (format!("{name}?"), lua_ty(inner)),
        t => (name.to_owned(), lua_ty(t)),
    }
}

fn lua_doc(w: &mut String, doc: &str) {
    for line in doc.lines() {
        let _ = writeln!(w, "--- {line}");
    }
}

fn gen_lua(m: &Model) -> String {
    let mut o = String::new();
    let w = &mut o;
    w.push_str(
        "---@meta\n\
         -- LuaLS stubs for the psi extension API. Generated from crates/psi-ext-api/api\n\
         -- by build.rs; do not edit. Type setup's argument with: ---@param psi psi\n",
    );

    if let Some(alias) = m.events_alias {
        w.push('\n');
        if let Some(t) = m.types.iter().find(|t| t.def.name == alias) {
            lua_doc(w, &t.def.doc);
        }
        let _ = writeln!(w, "---@alias {alias}");
        for e in &m.events {
            let _ = writeln!(w, "---| \"{}\" # {}", e.def.name, e.def.doc);
        }
    }

    for t in m.types.iter().filter(|t| t.def.alias.is_none()) {
        w.push('\n');
        lua_doc(w, &t.def.doc);
        let _ = writeln!(w, "---@class {}", t.def.name);
        for (f, ty) in &t.fields {
            let (name, ty) = lua_name_ty(&f.name, ty);
            let doc = if f.doc.is_empty() {
                String::new()
            } else {
                format!(" {}", f.doc)
            };
            let _ = writeln!(w, "---@field {name} {ty}{doc}");
        }
    }

    w.push_str("\n---@class psi\nlocal psi = {}\n");
    for t in m.tables() {
        let _ = writeln!(w, "\n---@class psi.{t}\npsi.{t} = {{}}");
    }

    for f in &m.funcs {
        w.push('\n');
        lua_doc(w, &f.def.doc);
        if f.def.is_async {
            w.push_str("---@async\n");
        }
        for (name, t) in &f.params {
            let (name, ty) = lua_name_ty(name, t);
            let _ = writeln!(w, "---@param {name} {ty}");
        }
        if let Some(r) = &f.ret {
            let _ = writeln!(w, "---@return {}", lua_ty(r));
        }
        lua_overloads(w, m, f);
        let names: Vec<&str> = f.params.iter().map(|(n, _)| *n).collect();
        let _ = writeln!(w, "function psi.{}({}) end", f.def.name, names.join(", "));
    }
    o
}

/// For a function taking an event name and then a handler (`psi.on`), one
/// overload per event typing the handler's first parameter and return.
fn lua_overloads(w: &mut String, m: &Model, f: &Func) {
    let Some(ev_at) = f.params.iter().position(|(_, t)| m.is_events(t)) else {
        return;
    };
    let Some(h_at) = f.params[ev_at..]
        .iter()
        .position(|(_, t)| matches!(t, Ty::Function { params, .. } if !params.is_empty()))
        .map(|i| i + ev_at)
    else {
        return;
    };
    for e in &m.events {
        let ps: Vec<String> = f
            .params
            .iter()
            .enumerate()
            .map(|(i, (name, t))| {
                let t = if i == ev_at {
                    Ty::Literals(vec![e.def.name.clone()])
                } else if i == h_at {
                    let Ty::Function { params, .. } = t else {
                        unreachable!("h_at is a function param");
                    };
                    let mut params = params.clone();
                    params[0].1 = e.payload.clone();
                    Ty::Function {
                        params,
                        ret: e.result.clone().map(Box::new),
                    }
                } else {
                    t.clone()
                };
                let (name, ty) = lua_name_ty(name, &t);
                format!("{name}: {ty}")
            })
            .collect();
        let _ = writeln!(w, "---@overload fun({})", ps.join(", "));
    }
}

// ---- Markdown --------------------------------------------------------------

/// Inline code in a table cell (pipes escaped).
fn cell(s: &str) -> String {
    format!("`{}`", s.replace('|', "\\|"))
}

fn text_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

fn gen_markdown(m: &Model) -> String {
    let mut o = String::new();
    let w = &mut o;
    w.push_str(
        "# Psi extension API\n\n\
         <!-- Generated from crates/psi-ext-api/api by build.rs; do not edit. -->\n\n\
         An extension's `setup(psi)` gets the `psi` table below. Types use the API's\n\
         grammar: `T?` is optional, `T[]` a list, `{K: V}` a map, `\"a\"|\"b\"` one of those\n\
         strings and `function(name: T): R` a function; other names are the types at the\n\
         end. Editor completion: the LuaLS stubs in `docs/api/psi.lua`.\n\n\
         ## Functions\n\n\
         An argument of the wrong type is an error naming the function and argument.\n\
         Functions marked *async* wait for their answer, suspending the calling handler\n\
         (not psi).\n",
    );
    let mut tables = vec![None];
    tables.extend(m.tables().into_iter().map(Some));
    for table in tables {
        let title = table.map_or("psi".to_owned(), |t| format!("psi.{t}"));
        let _ = writeln!(w, "\n### {title}");
        for f in m.funcs.iter().filter(|f| f.table == table) {
            let _ = writeln!(w, "\n#### psi.{}\n", f.def.name);
            let tag = if f.def.is_async { " -- async" } else { "" };
            let _ = writeln!(w, "```\n{}{tag}\n```\n", signature(f));
            let _ = writeln!(w, "{}", f.def.doc);
            if !f.def.params.is_empty() {
                w.push_str("\n| Param | Type | Optional |\n|---|---|---|\n");
                for (p, (_, t)) in f.def.params.iter().zip(&f.params) {
                    let opt = if matches!(t, Ty::Optional(_)) {
                        "yes"
                    } else {
                        ""
                    };
                    let _ = writeln!(w, "| `{}` | {} | {opt} |", p.name, cell(&p.ty));
                }
            }
            if let Some(r) = &f.def.returns {
                let _ = writeln!(w, "\nReturns {}.", cell(r));
            }
        }
    }

    w.push_str("\n## Events\n\nSubscribe with `psi.on(event, function(ev, ctx) ... end)`; `ev` is the payload and `ctx` a [`Context`](#context). Kinds:\n\n");
    for (k, doc) in KINDS {
        let _ = writeln!(w, "- **{k}**: {doc}");
    }
    w.push_str("\n| Event | Kind | Payload | Result | Description |\n|---|---|---|---|---|\n");
    for e in &m.events {
        let d = e.def;
        let result = d.result.as_deref().map(cell).unwrap_or_default();
        let _ = writeln!(
            w,
            "| `{}` | {} | {} | {result} | {} |",
            d.name,
            d.kind,
            cell(&d.payload),
            text_cell(&d.doc)
        );
    }

    w.push_str("\n## Types\n");
    for t in &m.types {
        let d = t.def;
        let _ = writeln!(w, "\n### {}\n", d.name);
        if !d.doc.is_empty() {
            let _ = writeln!(w, "{}\n", d.doc);
        }
        if d.alias.as_deref() == Some("events") {
            let names: Vec<String> = m
                .events
                .iter()
                .map(|e| format!("`\"{}\"`", e.def.name))
                .collect();
            let _ = writeln!(w, "One of: {}.", names.join(", "));
            continue;
        }
        w.push_str("| Field | Type | Description |\n|---|---|---|\n");
        for f in &d.fields {
            let _ = writeln!(
                w,
                "| `{}` | {} | {} |",
                f.name,
                cell(&f.ty),
                text_cell(&f.doc)
            );
        }
    }
    o
}
