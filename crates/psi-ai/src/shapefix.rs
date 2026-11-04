//! The tool-call shape-fix pipeline (psi-plan.md §4.4, D-09).
//!
//! On llama-server, tool-call failures are wrong shapes in valid JSON. A
//! call whose meaning is clear is accepted; one that can't be fixed safely
//! is refused with the exact fix and one correct example.

use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use crate::schema::{ParamKind, ToolSchema};
use crate::types::RawToolCall;

/// A call ready to run, and what was fixed on the way (for the record).
#[derive(Debug, Clone, PartialEq)]
pub struct FixedCall {
    pub name: String,
    pub args: Map<String, Value>,
    pub notes: Vec<String>,
}

pub const TRUNCATED: &str = "This call was cut off by the output token limit and was not run; its arguments may be incomplete. Resend a shorter call (for example, write a large file in parts).";

/// Tool names models use for the core tools.
const NAME_ALIASES: &[(&str, &[&str])] = &[
    (
        "read",
        &[
            "readfile", "view", "cat", "openfile", "viewfile", "fileread",
        ],
    ),
    (
        "write",
        &["writefile", "createfile", "filewrite", "create", "save"],
    ),
    (
        "edit",
        &[
            "strreplace",
            "replace",
            "editfile",
            "strreplaceeditor",
            "replaceinfile",
            "apply",
        ],
    ),
    (
        "bash",
        &[
            "run",
            "shell",
            "exec",
            "execute",
            "terminal",
            "runcommand",
            "command",
            "sh",
            "runshell",
        ],
    ),
    ("grep", &["search", "rg", "ripgrep", "searchfiles"]),
    ("find", &["glob", "findfiles"]),
    ("ls", &["list", "listdir", "listfiles", "dir"]),
    ("todo", &["todos", "todowrite", "todolist", "tasks", "task"]),
];

/// Argument names models use, by canonical parameter.
const KEY_ALIASES: &[(&str, &[&str])] = &[
    (
        "path",
        &[
            "filepath",
            "file",
            "filename",
            "pathname",
            "targetfile",
            "absolutepath",
            "dirpath",
            "directory",
        ],
    ),
    ("command", &["cmd", "script", "bashcommand", "shellcommand"]),
    (
        "old_text",
        &[
            "oldstring",
            "oldstr",
            "old",
            "search",
            "find",
            "original",
            "from",
        ],
    ),
    (
        "new_text",
        &[
            "newstring",
            "newstr",
            "new",
            "replace",
            "replacement",
            "to",
            "updated",
        ],
    ),
    (
        "text",
        &[
            "content",
            "contents",
            "filetext",
            "data",
            "body",
            "code",
            "filecontent",
        ],
    ),
    (
        "offset",
        &[
            "startline",
            "linestart",
            "fromline",
            "start",
            "line",
            "lineoffset",
        ],
    ),
    (
        "limit",
        &["lines", "maxlines", "count", "numlines", "linecount"],
    ),
    (
        "timeout",
        &["timeoutseconds", "timeoutsecs", "seconds", "timeouts"],
    ),
    (
        "intent",
        &["description", "reason", "purpose", "explanation"],
    ),
    ("pattern", &["query", "regex", "glob", "search"]),
];

/// Wrappers some models put the real arguments in.
const WRAPPERS: &[&str] = &["arguments", "args", "input", "parameters", "params"];

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Runs the pipeline on one call. `truncated` is true when the response hit
/// the output limit and this is its last call.
pub fn fix_call(
    call: &RawToolCall,
    truncated: bool,
    tools: &[ToolSchema],
    cwd: &Path,
    home: Option<&Path>,
) -> Result<FixedCall, String> {
    if truncated {
        return Err(TRUNCATED.into());
    }
    let mut notes = Vec::new();
    let tool = resolve_name(&call.name, tools, &mut notes)?;
    let mut args = parse_args(&call.arguments, tool, &mut notes)?;
    unwrap_wrapper(&mut args, tool, &mut notes);
    if tool.param("old_text").is_some() && tool.param("new_text").is_some() {
        fold_edit_list(&mut args, tool, &mut notes)?;
    }
    rename_keys(&mut args, tool, &mut notes);
    coerce(&mut args, tool, &mut notes)?;
    let mut missing: Vec<&str> = tool
        .params
        .iter()
        .filter(|p| p.required && !args.contains_key(p.name))
        .map(|p| p.name)
        .collect();
    // edit: old_text and new_text are needed unless edits holds the changes.
    if tool.param("edits").is_some() && !args.contains_key("edits") {
        for p in ["old_text", "new_text"] {
            if tool.param(p).is_some() && !args.contains_key(p) {
                missing.push(p);
            }
        }
    }
    // An edit with new_text and no old_text is usually a whole-file rewrite
    // (Gemma sent 15 in a row, 2026-11-01). The text may be a fragment, so
    // don't turn it into a write; say which tool does that.
    if missing == ["old_text"]
        && args.contains_key("new_text")
        && tools.iter().any(|t| t.name == "write")
    {
        let path = args.get("path").and_then(Value::as_str).unwrap_or("app.py");
        let write = serde_json::json!({"path": path, "text": "..."});
        return Err(format!(
            "{} is missing 'old_text', the exact text to replace. To replace the whole file, use write {write}. To change part of it: {} {}",
            tool.name, tool.name, tool.example
        ));
    }
    if !missing.is_empty() {
        let list = missing
            .iter()
            .map(|m| format!("'{m}'"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "{} is missing {list}. {}",
            tool.name,
            tool.example_line()
        ));
    }
    for p in tool.params.iter().filter(|p| p.is_path) {
        let Some(Value::String(path)) = args.get(p.name) else {
            continue;
        };
        let mut path = path.clone();
        if let (Some(rest), Some(home)) = (path.strip_prefix('~'), home)
            && (rest.is_empty() || rest.starts_with('/'))
        {
            path = format!("{}{rest}", home.display());
            notes.push(format!("expanded ~ in {}", p.name));
        }
        if let Some(abs) = doubled_cwd(&path, cwd) {
            return Err(format!(
                "'{path}' repeats the working directory ({}). Use {} instead.",
                cwd.display(),
                abs.display()
            ));
        }
        args.insert(p.name.into(), Value::String(path));
    }
    Ok(FixedCall {
        name: tool.name.into(),
        args,
        notes,
    })
}

fn resolve_name<'t>(
    name: &str,
    tools: &'t [ToolSchema],
    notes: &mut Vec<String>,
) -> Result<&'t ToolSchema, String> {
    if let Some(t) = tools.iter().find(|t| t.name == name) {
        return Ok(t);
    }
    let n = norm(name.rsplit('.').next().unwrap_or(name));
    let found = tools.iter().find(|t| norm(t.name) == n).or_else(|| {
        NAME_ALIASES
            .iter()
            .find(|(_, aliases)| aliases.contains(&n.as_str()))
            .and_then(|(canon, _)| tools.iter().find(|t| t.name == *canon))
    });
    let found = found.or_else(|| {
        let close: Vec<&ToolSchema> = tools
            .iter()
            .filter(|t| strsim::levenshtein(&n, t.name) <= 2)
            .collect();
        (close.len() == 1).then(|| close[0])
    });
    match found {
        Some(t) => {
            notes.push(format!("tool name '{name}' read as '{}'", t.name));
            Ok(t)
        }
        None => {
            let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
            Err(format!(
                "There is no tool named '{name}'. The tools are: {}.",
                names.join(", ")
            ))
        }
    }
}

fn parse_args(
    raw: &str,
    tool: &ToolSchema,
    notes: &mut Vec<String>,
) -> Result<Map<String, Value>, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Map::new());
    }
    let v: Value = serde_json::from_str(raw).map_err(|e| {
        format!(
            "The arguments for {} are not valid JSON ({e}). {}",
            tool.name,
            tool.example_line()
        )
    })?;
    let v = match v {
        // Arguments encoded twice: a JSON string holding the object.
        Value::String(s) => match serde_json::from_str::<Value>(&s) {
            Ok(inner @ Value::Object(_)) => {
                notes.push("arguments were a JSON string; decoded".into());
                inner
            }
            _ => Value::String(s),
        },
        v => v,
    };
    match v {
        Value::Object(m) => Ok(m),
        _ => Err(format!(
            "The arguments for {} must be a JSON object. {}",
            tool.name,
            tool.example_line()
        )),
    }
}

fn unwrap_wrapper(args: &mut Map<String, Value>, tool: &ToolSchema, notes: &mut Vec<String>) {
    if args.len() != 1 || tool.params.iter().any(|p| args.contains_key(p.name)) {
        return;
    }
    let Some(key) = args.keys().next().cloned() else {
        return;
    };
    if !WRAPPERS.contains(&norm(&key).as_str()) {
        return;
    }
    if let Some(Value::Object(inner)) = args.remove(&key) {
        notes.push(format!("arguments were wrapped in '{key}'; unwrapped"));
        *args = inner;
    }
}

/// pi-style `edits: [{oldText, newText}]` (seen from Gemma 4 in pi trials):
/// one real replacement is accepted as old_text/new_text; several become
/// `edits` with Psi's names, where the tool takes it; a top-level pair
/// whose old and new text are equal is a no-op and dropped. A list sent as
/// a JSON string, or one edit sent as an object, is read as the list.
fn fold_edit_list(
    args: &mut Map<String, Value>,
    tool: &ToolSchema,
    notes: &mut Vec<String>,
) -> Result<(), String> {
    let edits = match args.get("edits").cloned() {
        Some(Value::Array(a)) => a,
        Some(Value::String(s)) => match serde_json::from_str::<Value>(&s) {
            Ok(Value::Array(a)) => a,
            Ok(o @ Value::Object(_)) => vec![o],
            _ => return Ok(()),
        },
        Some(o @ Value::Object(_)) => vec![o],
        _ => return Ok(()),
    };
    let pick = |m: &Map<String, Value>, names: &[&str]| -> Option<Value> {
        m.iter()
            .find(|(k, _)| names.contains(&norm(k).as_str()))
            .map(|(_, v)| v.clone())
    };
    let olds = ["oldtext", "oldstring", "old"];
    let news = ["newtext", "newstring", "new"];
    let mut real = Vec::new();
    for (i, e) in edits.iter().enumerate() {
        let Value::Object(m) = e else {
            return Err(format!(
                "{}: edits[{i}] must be an object with old_text and new_text. {}",
                tool.name,
                tool.example_line()
            ));
        };
        match (pick(m, &olds), pick(m, &news)) {
            (Some(o), Some(n)) if o != n => {
                real.push((o, n, pick(m, &["path", "filepath", "file"])))
            }
            (Some(_), Some(_)) => {}
            _ => {
                return Err(format!(
                    "{}: edits[{i}] needs old_text and new_text. {}",
                    tool.name,
                    tool.example_line()
                ));
            }
        }
    }
    let top = (pick(args, &olds), pick(args, &news));
    let top_is_noop = matches!(&top, (Some(o), Some(n)) if o == n);
    if let (Some(o), Some(n)) = top.clone()
        && !top_is_noop
    {
        real.push((o, n, None));
    }
    if real.len() > 1 {
        if tool.param("edits").is_none() {
            return Err(format!(
                "edit makes one replacement per call, given as old_text and new_text. Make one call per change. {}",
                tool.example_line()
            ));
        }
        let path = real.iter().find_map(|(_, _, p)| p.clone());
        let list: Vec<Value> = real
            .into_iter()
            .map(|(o, n, _)| serde_json::json!({"old_text": o, "new_text": n}))
            .collect();
        let canonical = args.get("edits") == Some(&Value::Array(list.clone()));
        args.retain(|k, _| {
            let k = norm(k);
            k != "edits" && !olds.contains(&k.as_str()) && !news.contains(&k.as_str())
        });
        args.insert("edits".into(), Value::Array(list));
        if let Some(p) = path
            && !args
                .keys()
                .any(|k| ["path", "filepath", "file"].contains(&norm(k).as_str()))
        {
            args.insert("path".into(), p);
        }
        if !canonical {
            notes.push("edit: read the replacements in 'edits' with Psi's names".into());
        }
        return Ok(());
    }
    let Some((o, n, path)) = real.pop() else {
        return Ok(());
    };
    args.retain(|k, _| {
        let k = norm(k);
        k != "edits" && !olds.contains(&k.as_str()) && !news.contains(&k.as_str())
    });
    args.insert("old_text".into(), o);
    args.insert("new_text".into(), n);
    if let Some(p) = path
        && !args
            .keys()
            .any(|k| ["path", "filepath", "file"].contains(&norm(k).as_str()))
    {
        args.insert("path".into(), p);
    }
    notes.push(if top_is_noop {
        "edit: used the one replacement in 'edits' and dropped a top-level pair that changed nothing".into()
    } else {
        "edit: took the one replacement from 'edits'".into()
    });
    Ok(())
}

fn rename_keys(args: &mut Map<String, Value>, tool: &ToolSchema, notes: &mut Vec<String>) {
    let keys: Vec<String> = args.keys().cloned().collect();
    for key in keys {
        if tool.param(&key).is_some() {
            continue;
        }
        let k = norm(&key);
        let target = tool.params.iter().find(|p| {
            !args.contains_key(p.name)
                && (norm(p.name) == k
                    || KEY_ALIASES
                        .iter()
                        .any(|(canon, al)| *canon == p.name && al.contains(&k.as_str())))
        });
        match target {
            Some(p) => {
                if let Some(v) = args.remove(&key) {
                    args.insert(p.name.into(), v);
                    notes.push(format!("argument '{key}' read as '{}'", p.name));
                }
            }
            None => {
                args.remove(&key);
                notes.push(format!("ignored unknown argument '{key}'"));
            }
        }
    }
}

fn coerce(
    args: &mut Map<String, Value>,
    tool: &ToolSchema,
    notes: &mut Vec<String>,
) -> Result<(), String> {
    for p in &tool.params {
        let Some(v) = args.get(p.name).cloned() else {
            continue;
        };
        let fixed = match (p.kind, &v) {
            // A null is the same as leaving the argument out.
            (_, Value::Null) => {
                args.remove(p.name);
                continue;
            }
            (ParamKind::Any, _)
            | (ParamKind::String, Value::String(_))
            | (ParamKind::Boolean, Value::Bool(_))
            | (ParamKind::Number, Value::Number(_))
            | (ParamKind::Array, Value::Array(_))
            | (ParamKind::Object, Value::Object(_)) => {
                continue;
            }
            (ParamKind::Number, Value::String(s)) => s
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number),
            (ParamKind::Integer, Value::Number(n)) if n.is_i64() || n.is_u64() => continue,
            // 10.0 is 10; 10.5 is a mistake.
            (ParamKind::Integer, Value::Number(n)) => n
                .as_f64()
                .filter(|f| f.fract() == 0.0)
                .map(|f| Value::from(f as i64)),
            (ParamKind::Integer, Value::String(s)) => s.trim().parse::<i64>().ok().map(Value::from),
            (ParamKind::Boolean, Value::String(s)) => {
                match s.trim().to_ascii_lowercase().as_str() {
                    "true" => Some(Value::Bool(true)),
                    "false" => Some(Value::Bool(false)),
                    _ => None,
                }
            }
            (ParamKind::String, Value::Number(n)) => Some(Value::String(n.to_string())),
            _ => None,
        };
        let Some(f) = fixed else {
            let want = match p.kind {
                ParamKind::String => "a string",
                ParamKind::Integer => "an integer",
                ParamKind::Boolean => "true or false",
                ParamKind::Number => "a number",
                ParamKind::Array => "a list",
                ParamKind::Object => "an object",
                ParamKind::Any => "a value",
            };
            return Err(format!(
                "{}: '{}' must be {want}. {}",
                tool.name,
                p.name,
                tool.example_line()
            ));
        };
        notes.push(format!("{} coerced to {:?}", p.name, p.kind));
        args.insert(p.name.into(), f);
    }
    Ok(())
}

/// A relative path that starts with the working directory's own trailing
/// components (`work/project/x` from `/work/project`) and only makes sense
/// with them removed. Returns the absolute path to use instead.
fn doubled_cwd(path: &str, cwd: &Path) -> Option<PathBuf> {
    if Path::new(path).is_absolute() || cwd.join(path).exists() {
        return None;
    }
    let comps: Vec<String> = cwd
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    for k in (1..=comps.len()).rev() {
        let tail = comps[comps.len() - k..].join("/");
        let Some(rest) = path.strip_prefix(&format!("{tail}/")) else {
            continue;
        };
        let fixed = cwd.join(rest);
        if fixed.exists() || fixed.parent().is_some_and(Path::exists) {
            return Some(fixed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Param;
    use serde_json::json;

    fn tools() -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "read",
                description: String::new(),
                params: vec![
                    Param::new("path", ParamKind::String, "").path(),
                    Param::new("offset", ParamKind::Integer, "").optional(),
                    Param::new("limit", ParamKind::Integer, "").optional(),
                ],
                example: json!({"path": "src/app.py"}),
            },
            ToolSchema {
                name: "edit",
                description: String::new(),
                // As psi-tools has it: old_text and new_text, or edits.
                params: vec![
                    Param::new("path", ParamKind::String, "").path(),
                    Param::new("old_text", ParamKind::String, "").optional(),
                    Param::new("new_text", ParamKind::String, "").optional(),
                    Param::new("edits", ParamKind::Array, "").optional(),
                    Param::new("intent", ParamKind::String, "").optional(),
                ],
                example: json!({"path": "app.py", "old_text": "x = 1", "new_text": "x = 2"}),
            },
            ToolSchema {
                name: "bash",
                description: String::new(),
                params: vec![
                    Param::new("command", ParamKind::String, ""),
                    Param::new("timeout", ParamKind::Integer, "").optional(),
                ],
                example: json!({"command": "ls"}),
            },
        ]
    }

    fn fix(name: &str, args: &str) -> Result<FixedCall, String> {
        let call = RawToolCall {
            id: "c".into(),
            name: name.into(),
            arguments: args.into(),
        };
        fix_call(
            &call,
            false,
            &tools(),
            Path::new("/nonexistent/work/project"),
            Some(Path::new("/home/u")),
        )
    }

    #[test]
    fn exact_call_passes_untouched() {
        let f = fix("read", r#"{"path": "a.py", "offset": 3}"#).unwrap();
        assert_eq!(
            f.args,
            json!({"path": "a.py", "offset": 3})
                .as_object()
                .cloned()
                .unwrap()
        );
        assert!(f.notes.is_empty());
    }

    #[test]
    fn truncated_calls_never_run() {
        let call = RawToolCall {
            id: "c".into(),
            name: "read".into(),
            arguments: "{\"path\": \"a".into(),
        };
        assert_eq!(
            fix_call(&call, true, &tools(), Path::new("/"), None).unwrap_err(),
            TRUNCATED
        );
    }

    #[test]
    fn names_resolve_by_alias_and_distance() {
        assert_eq!(fix("read_file", r#"{"path": "a"}"#).unwrap().name, "read");
        assert_eq!(
            fix(
                "functions.str_replace",
                r#"{"path":"a","old_text":"x","new_text":"y"}"#
            )
            .unwrap()
            .name,
            "edit"
        );
        assert_eq!(fix("bsh", r#"{"command": "ls"}"#).unwrap().name, "bash");
        let err = fix("browse", "{}").unwrap_err();
        assert_eq!(
            err,
            "There is no tool named 'browse'. The tools are: read, edit, bash."
        );
    }

    #[test]
    fn clear_mistakes_are_accepted() {
        let f = fix(
            "read",
            r#"{"file_path": "a.py", "offset": "10", "limit": 5.0}"#,
        )
        .unwrap();
        assert_eq!(
            f.args,
            json!({"path": "a.py", "offset": 10, "limit": 5})
                .as_object()
                .cloned()
                .unwrap()
        );
        let f = fix("bash", r#"{"arguments": {"cmd": "ls"}}"#).unwrap();
        assert_eq!(f.args["command"], "ls");
        let f = fix("bash", r#""{\"command\": \"ls\"}""#).unwrap();
        assert_eq!(f.args["command"], "ls");
        let f = fix("read", r#"{"path": "~/notes.md"}"#).unwrap();
        assert_eq!(f.args["path"], "/home/u/notes.md");
    }

    #[test]
    fn unknown_keys_are_dropped_and_description_becomes_intent() {
        let f = fix(
            "edit",
            r#"{"path":"a","old_text":"x","new_text":"y","description":"fix","color":1}"#,
        )
        .unwrap();
        assert_eq!(f.args["intent"], "fix");
        assert!(!f.args.contains_key("color"));
    }

    #[test]
    fn refusals_give_one_example() {
        let err = fix("edit", r#"{"old_text": "x", "new_text": "y"}"#).unwrap_err();
        assert_eq!(
            err,
            r#"edit is missing 'path'. Example: edit {"path":"app.py","old_text":"x = 1","new_text":"x = 2"}"#
        );
        let err = fix("read", r#"{"path": "a", "offset": "ten"}"#).unwrap_err();
        assert!(
            err.starts_with("read: 'offset' must be an integer."),
            "{err}"
        );
        let err = fix("read", "{path: a}").unwrap_err();
        assert!(
            err.starts_with("The arguments for read are not valid JSON"),
            "{err}"
        );
    }

    #[test]
    fn edit_without_old_text_points_at_write() {
        // The shape Gemma 4 12B sent 15 times (2026-11-01, feat_logstats_jsonl).
        let mut ts = tools();
        ts.push(ToolSchema {
            name: "write",
            description: String::new(),
            params: vec![
                Param::new("path", ParamKind::String, "").path(),
                Param::new("text", ParamKind::String, ""),
            ],
            example: json!({"path": "notes.md", "text": "x"}),
        });
        let call = RawToolCall {
            id: "c".into(),
            name: "edit".into(),
            arguments: r#"{"intent":"add json","new_text":"import re\nimport json\n","path":"logstats/parse.py"}"#.into(),
        };
        let err = fix_call(&call, false, &ts, Path::new("/nonexistent"), None).unwrap_err();
        assert_eq!(
            err,
            r#"edit is missing 'old_text', the exact text to replace. To replace the whole file, use write {"path":"logstats/parse.py","text":"..."}. To change part of it: edit {"path":"app.py","old_text":"x = 1","new_text":"x = 2"}"#
        );
        // Without a write tool, the usual refusal.
        let err = fix("edit", r#"{"path":"a.py","new_text":"y"}"#).unwrap_err();
        assert!(
            err.starts_with("edit is missing 'old_text'. Example:"),
            "{err}"
        );
    }

    #[test]
    fn several_edits_pass_with_psis_names() {
        // pi's names, several replacements: Psi's names, as one list.
        let c = fix(
            "edit",
            r#"{"path":"a.py","edits":[{"oldText":"a","newText":"b"},{"oldText":"c","newText":"d"},{"oldText":"e","newText":"e"}]}"#,
        )
        .unwrap();
        assert_eq!(
            c.args["edits"],
            json!([{"old_text": "a", "new_text": "b"}, {"old_text": "c", "new_text": "d"}])
        );
        assert!(!c.args.contains_key("old_text"));
        assert_eq!(
            c.notes,
            ["edit: read the replacements in 'edits' with Psi's names"]
        );
        // Already in Psi's form: untouched, no note.
        let c = fix(
            "edit",
            r#"{"path":"a.py","edits":[{"old_text":"a","new_text":"b"},{"old_text":"c","new_text":"d"}]}"#,
        )
        .unwrap();
        assert!(c.notes.is_empty(), "{:?}", c.notes);
        // A list sent as a JSON string is read as the list.
        let c = fix(
            "edit",
            r#"{"path":"a.py","edits":"[{\"old_text\":\"a\",\"new_text\":\"b\"},{\"old_text\":\"c\",\"new_text\":\"d\"}]"}"#,
        )
        .unwrap();
        assert_eq!(c.args["edits"].as_array().unwrap().len(), 2);
        // One edit in the list becomes old_text and new_text, as before.
        let c = fix(
            "edit",
            r#"{"path":"a.py","edits":[{"old_text":"a","new_text":"b"}]}"#,
        )
        .unwrap();
        assert_eq!(
            (c.args["old_text"].as_str(), c.args.contains_key("edits")),
            (Some("a"), false)
        );
        // An item without new_text is refused, not dropped.
        let e = fix(
            "edit",
            r#"{"path":"a.py","edits":[{"old_text":"a","new_text":"b"},{"old_text":"c"}]}"#,
        )
        .unwrap_err();
        assert!(
            e.starts_with("edit: edits[1] needs old_text and new_text."),
            "{e}"
        );
        // Neither form: old_text and new_text are missing.
        let e = fix("edit", r#"{"path":"a.py"}"#).unwrap_err();
        assert!(
            e.starts_with("edit is missing 'old_text', 'new_text'."),
            "{e}"
        );
    }

    #[test]
    fn doubled_working_directory_is_refused_with_the_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("work/project");
        std::fs::create_dir_all(cwd.join("src")).unwrap();
        std::fs::write(cwd.join("src/a.py"), "").unwrap();
        let call = |p: &str| RawToolCall {
            id: "c".into(),
            name: "read".into(),
            arguments: json!({"path": p}).to_string(),
        };
        let err =
            fix_call(&call("work/project/src/a.py"), false, &tools(), &cwd, None).unwrap_err();
        assert_eq!(
            err,
            format!(
                "'work/project/src/a.py' repeats the working directory ({}). Use {} instead.",
                cwd.display(),
                cwd.join("src/a.py").display()
            )
        );
        assert!(fix_call(&call("project/src/a.py"), false, &tools(), &cwd, None).is_err());
        // A real subdirectory with that name is fine.
        std::fs::create_dir_all(cwd.join("project/src")).unwrap();
        std::fs::write(cwd.join("project/src/a.py"), "").unwrap();
        assert!(fix_call(&call("project/src/a.py"), false, &tools(), &cwd, None).is_ok());
        assert!(fix_call(&call("src/a.py"), false, &tools(), &cwd, None).is_ok());
    }
}
