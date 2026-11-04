//! The builtin batteries (psi-plan.md §9.4, D-45): `psi.json`, `psi.re`,
//! `psi.path`, `psi.fs`, `psi.toml`, `psi.base64`, `psi.hash` and
//! `psi.time`, implemented once as the default methods of `ExtHost`, so
//! every host (psi, `psi ext check`, `psi ext test`) has the same ones.
//! Strings arrive and leave as Lua strings, so bytes pass through.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use mlua::{Lua, LuaSerdeExt, Value};
use regex::Regex;
use serde_json::Value as Json;
use sha2::Digest as _;

use crate::glue::error;

/// A path or pattern argument as text.
fn text(s: &mlua::LuaString) -> String {
    s.to_string_lossy()
}

/// An mlua error's message without its "runtime error:" or callback wrapping.
fn message(e: &mlua::Error) -> String {
    match e {
        mlua::Error::CallbackError { cause, .. } => message(cause),
        mlua::Error::RuntimeError(m) => m.clone(),
        mlua::Error::SerializeError(m) | mlua::Error::DeserializeError(m) => m.clone(),
        other => other.to_string(),
    }
}

/// What `io.popen` and `os.execute` say instead of running a command: they
/// would bypass the sandbox and the always-ask rules (D-45).
pub const NO_SHELL: &str = "is not available in psi extensions: use psi.exec(argv, opts), which runs inside the sandbox and asks for commands on the always-ask list";

/// Replaces `io.popen` and `os.execute` with functions that raise
/// [`NO_SHELL`], so a call says what to use instead.
pub fn remove_shell(lua: &Lua) -> mlua::Result<()> {
    for (lib, name) in [("io", "popen"), ("os", "execute")] {
        let Ok(t) = lua.globals().get::<mlua::Table>(lib) else {
            continue;
        };
        let msg = format!("{lib}.{name} {NO_SHELL}");
        t.set(
            name,
            lua.create_function(move |_, _: mlua::MultiValue| -> mlua::Result<()> {
                Err(error(msg.clone()))
            })?,
        )?;
    }
    Ok(())
}

// ---- JSON ------------------------------------------------------------------

/// Sorts object keys at every level: Lua tables have no order, so this
/// keeps the output the same from run to run.
fn sort_keys(v: &mut Json) {
    match v {
        Json::Object(m) => {
            let mut entries: Vec<(String, Json)> = std::mem::take(m).into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (_, x) in &mut entries {
                sort_keys(x);
            }
            *m = entries.into_iter().collect();
        }
        Json::Array(a) => a.iter_mut().for_each(sort_keys),
        _ => {}
    }
}

/// A Lua value as JSON, keys sorted; `func` names the caller in errors.
pub fn to_json(lua: &Lua, func: &str, value: Value) -> mlua::Result<Json> {
    let mut j: Json = lua
        .from_value(value)
        .map_err(|e| error(format!("{func}: {}", message(&e))))?;
    sort_keys(&mut j);
    Ok(j)
}

pub(crate) fn json_encode(
    lua: &Lua,
    value: Value,
    opts: Option<Value>,
) -> mlua::Result<mlua::LuaString> {
    let pretty = match &opts {
        Some(Value::Table(t)) => t.get::<Option<bool>>("pretty")?.unwrap_or(false),
        _ => false,
    };
    let j = to_json(lua, "psi.json.encode", value)?;
    let s = if pretty {
        serde_json::to_string_pretty(&j)
    } else {
        serde_json::to_string(&j)
    }
    .map_err(|e| error(format!("psi.json.encode: {e}")))?;
    lua.create_string(s)
}

/// JSON text as a Lua value: null is `lua.null()`, arrays carry the array
/// metatable (so they encode back as arrays).
pub fn json_decode(lua: &Lua, text: mlua::LuaString) -> mlua::Result<Value> {
    let j: Json = serde_json::from_slice(&text.as_bytes())
        .map_err(|e| error(format!("psi.json.decode: {e}")))?;
    lua.to_value(&j)
}

pub(crate) fn json_null(lua: &Lua) -> mlua::Result<Value> {
    Ok(lua.null())
}

pub(crate) fn json_array(lua: &Lua, t: Value) -> mlua::Result<Value> {
    if let Value::Table(tbl) = &t {
        tbl.set_metatable(Some(lua.array_metatable()))?;
    }
    Ok(t)
}

// ---- Regular expressions ---------------------------------------------------

/// Compiled patterns, so a pattern used in a loop compiles once.
const CACHE_MAX: usize = 128;

thread_local! {
    static PATTERNS: RefCell<HashMap<String, Regex>> = RefCell::new(HashMap::new());
}

fn regex(func: &str, pattern: &mlua::LuaString) -> mlua::Result<Regex> {
    let pattern = text(pattern);
    PATTERNS.with_borrow_mut(|cache| {
        if let Some(r) = cache.get(&pattern) {
            return Ok(r.clone());
        }
        let r = Regex::new(&pattern).map_err(|e| error(format!("{func}: bad pattern: {e}")))?;
        if cache.len() >= CACHE_MAX {
            cache.clear();
        }
        cache.insert(pattern, r.clone());
        Ok(r)
    })
}

fn match_table(lua: &Lua, re: &Regex, caps: &regex::Captures) -> mlua::Result<Value> {
    let Some(m) = caps.get(0) else {
        return Ok(Value::Nil);
    };
    let t = lua.create_table()?;
    t.set("text", m.as_str())?;
    t.set("start", m.start() + 1)?;
    t.set("finish", m.end())?;
    let groups = lua.create_table()?;
    for i in 1..caps.len() {
        groups.raw_push(caps.get(i).map_or("", |g| g.as_str()))?;
    }
    t.set("groups", groups)?;
    let named = lua.create_table()?;
    for name in re.capture_names().flatten() {
        if let Some(g) = caps.name(name) {
            named.set(name, g.as_str())?;
        }
    }
    t.set("named", named)?;
    Ok(Value::Table(t))
}

pub(crate) fn re_is_match(
    _: &Lua,
    pattern: mlua::LuaString,
    text: mlua::LuaString,
) -> mlua::Result<bool> {
    let re = regex("psi.re.is_match", &pattern)?;
    Ok(re.is_match(&self::text(&text)))
}

pub(crate) fn re_find(
    lua: &Lua,
    pattern: mlua::LuaString,
    text: mlua::LuaString,
) -> mlua::Result<Option<Value>> {
    let re = regex("psi.re.find", &pattern)?;
    let s = self::text(&text);
    match re.captures(&s) {
        Some(c) => match_table(lua, &re, &c).map(Some),
        None => Ok(None),
    }
}

pub(crate) fn re_find_all(
    lua: &Lua,
    pattern: mlua::LuaString,
    text: mlua::LuaString,
) -> mlua::Result<Value> {
    let re = regex("psi.re.find_all", &pattern)?;
    let s = self::text(&text);
    let out = lua.create_table()?;
    for c in re.captures_iter(&s) {
        out.raw_push(match_table(lua, &re, &c)?)?;
    }
    Ok(Value::Table(out))
}

pub(crate) fn re_replace(
    lua: &Lua,
    pattern: mlua::LuaString,
    text: mlua::LuaString,
    replacement: mlua::LuaString,
    limit: Option<i64>,
) -> mlua::Result<mlua::LuaString> {
    let re = regex("psi.re.replace", &pattern)?;
    let s = self::text(&text);
    let limit = usize::try_from(limit.unwrap_or(0)).unwrap_or(0);
    let out = re.replacen(&s, limit, self::text(&replacement).as_str());
    lua.create_string(out.as_bytes())
}

pub(crate) fn re_split(
    _: &Lua,
    pattern: mlua::LuaString,
    text: mlua::LuaString,
) -> mlua::Result<Vec<String>> {
    let re = regex("psi.re.split", &pattern)?;
    Ok(re.split(&self::text(&text)).map(str::to_owned).collect())
}

pub(crate) fn re_escape(lua: &Lua, s: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    lua.create_string(regex::escape(&self::text(&s)).as_bytes())
}

// ---- Paths -----------------------------------------------------------------

fn out_path(lua: &Lua, p: &Path) -> mlua::Result<mlua::LuaString> {
    lua.create_string(p.to_string_lossy().as_bytes())
}

/// `.` and `..` resolved without looking at the disk; `..` above the root
/// stays at the root, and a relative path keeps its leading `..`.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            c => out.push(c),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

fn absolute(func: &str, p: &Path) -> mlua::Result<PathBuf> {
    if p.is_absolute() {
        return Ok(normalize(p));
    }
    let cwd = std::env::current_dir()
        .map_err(|e| error(format!("{func}: reading the working directory: {e}")))?;
    Ok(normalize(&cwd.join(p)))
}

pub(crate) fn path_join(
    lua: &Lua,
    base: mlua::LuaString,
    part: mlua::LuaString,
) -> mlua::Result<mlua::LuaString> {
    out_path(lua, &Path::new(&text(&base)).join(text(&part)))
}

pub(crate) fn path_normalize(lua: &Lua, p: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    out_path(lua, &normalize(Path::new(&text(&p))))
}

pub(crate) fn path_absolute(lua: &Lua, p: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    out_path(lua, &absolute("psi.path.absolute", Path::new(&text(&p)))?)
}

pub(crate) fn path_relative(
    lua: &Lua,
    p: mlua::LuaString,
    base: Option<mlua::LuaString>,
) -> mlua::Result<mlua::LuaString> {
    let func = "psi.path.relative";
    let p = absolute(func, Path::new(&text(&p)))?;
    let base = match base {
        Some(b) => absolute(func, Path::new(&text(&b)))?,
        None => absolute(func, Path::new("."))?,
    };
    let (pc, bc): (Vec<_>, Vec<_>) = (p.components().collect(), base.components().collect());
    let common = pc.iter().zip(&bc).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..bc.len() {
        out.push("..");
    }
    for c in &pc[common..] {
        out.push(c);
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out_path(lua, &out)
}

pub(crate) fn path_basename(lua: &Lua, p: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    let p = text(&p);
    let name = Path::new(&p)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    lua.create_string(name)
}

pub(crate) fn path_dirname(lua: &Lua, p: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    let p = text(&p);
    let dir = match Path::new(&p).parent() {
        Some(d) if d.as_os_str().is_empty() => PathBuf::from("."),
        Some(d) => d.to_owned(),
        None if p.starts_with('/') => PathBuf::from("/"),
        None => PathBuf::from("."),
    };
    out_path(lua, &dir)
}

pub(crate) fn path_ext(lua: &Lua, p: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    let p = text(&p);
    let ext = Path::new(&p)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    lua.create_string(ext)
}

// ---- Files -----------------------------------------------------------------

pub(crate) fn fs_read(lua: &Lua, path: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    let p = text(&path);
    let data = std::fs::read(&p).map_err(|e| error(format!("psi.fs.read: {p}: {e}")))?;
    lua.create_string(data)
}

pub(crate) fn fs_write(_: &Lua, path: mlua::LuaString, data: mlua::LuaString) -> mlua::Result<()> {
    let p = text(&path);
    let fail = |e: std::io::Error| error(format!("psi.fs.write: {p}: {e}"));
    if let Some(dir) = Path::new(&p).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    std::fs::write(&p, data.as_bytes()).map_err(fail)
}

pub(crate) fn fs_exists(_: &Lua, path: mlua::LuaString) -> mlua::Result<bool> {
    Ok(Path::new(&text(&path)).exists())
}

pub(crate) fn fs_stat(lua: &Lua, path: mlua::LuaString) -> mlua::Result<Option<Value>> {
    let Ok(m) = std::fs::metadata(text(&path)) else {
        return Ok(None);
    };
    let t = lua.create_table()?;
    let kind = if m.is_file() {
        "file"
    } else if m.is_dir() {
        "dir"
    } else {
        "other"
    };
    t.set("kind", kind)?;
    t.set("size", m.len())?;
    let modified = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64());
    t.set("modified", modified)?;
    Ok(Some(Value::Table(t)))
}

pub(crate) fn fs_list(_: &Lua, dir: mlua::LuaString) -> mlua::Result<Vec<String>> {
    let d = text(&dir);
    let entries = std::fs::read_dir(&d).map_err(|e| error(format!("psi.fs.list: {d}: {e}")))?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names)
}

/// More matches than this is an error, not a silent cut.
const GLOB_MAX: usize = 100_000;

pub(crate) fn fs_glob(
    _: &Lua,
    pattern: mlua::LuaString,
    opts: Option<Value>,
) -> mlua::Result<Vec<String>> {
    let func = "psi.fs.glob";
    let (cwd, gitignore) = match &opts {
        Some(Value::Table(t)) => (
            t.get::<Option<String>>("cwd")?,
            t.get::<Option<bool>>("gitignore")?.unwrap_or(true),
        ),
        _ => (None, true),
    };
    let root = absolute(func, Path::new(cwd.as_deref().unwrap_or(".")))?;
    let pattern = text(&pattern);
    let glob = globset::GlobBuilder::new(pattern.trim_start_matches("./"))
        .literal_separator(true)
        .build()
        .map_err(|e| error(format!("{func}: {e}")))?
        .compile_matcher();
    let walk = ignore::WalkBuilder::new(&root)
        .standard_filters(gitignore)
        .hidden(gitignore)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    let mut out = Vec::new();
    for entry in walk.filter_map(Result::ok) {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(&root) else {
            continue;
        };
        if glob.is_match(rel) {
            if out.len() == GLOB_MAX {
                return Err(error(format!(
                    "{func}: more than {GLOB_MAX} files match {pattern:?}; narrow the pattern"
                )));
            }
            out.push(rel.to_string_lossy().into_owned());
        }
    }
    out.sort();
    Ok(out)
}

// ---- Formats ---------------------------------------------------------------

fn toml_to_json(v: toml::Value) -> Json {
    match v {
        toml::Value::String(s) => Json::String(s),
        toml::Value::Integer(i) => Json::from(i),
        toml::Value::Float(f) => serde_json::Number::from_f64(f).map_or(Json::Null, Json::Number),
        toml::Value::Boolean(b) => Json::Bool(b),
        toml::Value::Datetime(d) => Json::String(d.to_string()),
        toml::Value::Array(a) => Json::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            Json::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect())
        }
    }
}

pub(crate) fn toml_decode(lua: &Lua, text: mlua::LuaString) -> mlua::Result<Value> {
    let t: toml::Table = toml::from_str(&self::text(&text))
        .map_err(|e| error(format!("psi.toml.decode: {}", e.to_string().trim_end())))?;
    lua.to_value(&toml_to_json(toml::Value::Table(t)))
}

pub(crate) fn toml_encode(lua: &Lua, t: Value) -> mlua::Result<mlua::LuaString> {
    let func = "psi.toml.encode";
    let j = to_json(lua, func, t)?;
    if !j.is_object() {
        return Err(error(format!(
            "{func}: a TOML document is a table with keys"
        )));
    }
    let s = toml::to_string(&j).map_err(|e| error(format!("{func}: {e}")))?;
    lua.create_string(s)
}

pub(crate) fn base64_encode(lua: &Lua, data: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    lua.create_string(base64::engine::general_purpose::STANDARD.encode(data.as_bytes()))
}

pub(crate) fn base64_decode(lua: &Lua, text: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.as_bytes().trim_ascii())
        .map_err(|e| error(format!("psi.base64.decode: {e}")))?;
    lua.create_string(bytes)
}

pub(crate) fn hash_sha256(lua: &Lua, data: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    let digest = sha2::Sha256::digest(data.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    lua.create_string(hex)
}

// ---- Time ------------------------------------------------------------------

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn seconds<Tz: chrono::TimeZone>(t: &chrono::DateTime<Tz>) -> f64 {
    t.timestamp() as f64 + f64::from(t.timestamp_subsec_nanos()) / 1e9
}

pub(crate) fn time_now(_: &Lua) -> mlua::Result<f64> {
    Ok(now())
}

pub(crate) fn time_format(
    lua: &Lua,
    t: Option<f64>,
    fmt: Option<mlua::LuaString>,
) -> mlua::Result<mlua::LuaString> {
    use chrono::format::{Item, StrftimeItems};
    use std::fmt::Write as _;
    let func = "psi.time.format";
    let t = t.unwrap_or_else(now);
    let fmt = fmt.map_or_else(|| "%Y-%m-%dT%H:%M:%S%:z".to_owned(), |f| text(&f));
    let items: Vec<Item> = StrftimeItems::new(&fmt).collect();
    if items.iter().any(|i| matches!(i, Item::Error)) {
        return Err(error(format!("{func}: bad format {fmt:?}")));
    }
    // The fraction is whole nanoseconds below one second.
    let nanos = ((t - t.floor()) * 1e9) as u32;
    let Some(utc) = chrono::DateTime::from_timestamp(t.floor() as i64, nanos.min(999_999_999))
    else {
        return Err(error(format!("{func}: {t} is out of range")));
    };
    let local = utc.with_timezone(&chrono::Local);
    let mut out = String::new();
    write!(out, "{}", local.format_with_items(items.iter()))
        .map_err(|_| error(format!("{func}: {fmt:?} doesn't apply to this time")))?;
    lua.create_string(out)
}

pub(crate) fn time_parse(
    _: &Lua,
    text: mlua::LuaString,
    fmt: Option<mlua::LuaString>,
) -> mlua::Result<Option<f64>> {
    use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
    let s = self::text(&text);
    let s = s.trim();
    let Some(fmt) = fmt.map(|f| self::text(&f)) else {
        return Ok(DateTime::parse_from_rfc3339(s).ok().map(|t| seconds(&t)));
    };
    if let Ok(t) = DateTime::parse_from_str(s, &fmt) {
        return Ok(Some(seconds(&t)));
    }
    let naive = NaiveDateTime::parse_from_str(s, &fmt).ok().or_else(|| {
        NaiveDate::parse_from_str(s, &fmt)
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
    });
    Ok(naive
        .and_then(|n| Local.from_local_datetime(&n).earliest())
        .map(|t| seconds(&t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_is_lexical() {
        let n = |s: &str| normalize(Path::new(s)).display().to_string();
        assert_eq!(n("a/./b/../c"), "a/c");
        assert_eq!(n("../a/.."), "..");
        assert_eq!(n("/../a"), "/a");
        assert_eq!(n("a/.."), ".");
    }

    #[test]
    fn keys_are_sorted_at_every_level() {
        let mut j: Json =
            serde_json::from_str(r#"{"b":1,"a":{"d":2,"c":[{"f":1,"e":0}]}}"#).unwrap_or_default();
        sort_keys(&mut j);
        assert_eq!(j.to_string(), r#"{"a":{"c":[{"e":0,"f":1}],"d":2},"b":1}"#);
    }
}
