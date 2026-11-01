//! `mcp.json`: the `mcpServers` format Claude Code, Cursor and pi share,
//! plus Psi's own fields (psi-plan.md §10.2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};

/// Seconds a call may take when the entry doesn't say.
pub const DEFAULT_TIMEOUT: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<PathBuf>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// When a call to one of the server's tools asks first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// Runs when the server marks the tool read-only; asks otherwise.
    Auto,
    Ask,
    /// Runs without asking.
    Never,
}

impl Approval {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "auto" => Self::Auto,
            "ask" => Self::Ask,
            "never" => Self::Never,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub name: String,
    pub transport: Transport,
    pub enabled: bool,
    /// Only these tools, when set.
    pub tools: Option<Vec<String>>,
    pub disabled_tools: Vec<String>,
    pub approval: Approval,
    /// Per tool, over `approval`.
    pub tool_approval: BTreeMap<String, Approval>,
    pub timeout: Duration,
    /// The file it came from.
    pub source: PathBuf,
}

impl ServerConfig {
    /// Whether the model is offered the server tool named `tool`.
    pub fn offers(&self, tool: &str) -> bool {
        self.tools
            .as_ref()
            .is_none_or(|t| t.iter().any(|n| n == tool))
            && !self.disabled_tools.iter().any(|n| n == tool)
    }

    pub fn approval_for(&self, tool: &str) -> Approval {
        self.tool_approval
            .get(tool)
            .copied()
            .unwrap_or(self.approval)
    }
}

/// The servers of the files given, later files first (a project file
/// replaces a user entry of the same name), and what was wrong with the
/// files or entries that were skipped. A missing file is no problem.
pub fn load(
    files: &[PathBuf],
    env: &dyn Fn(&str) -> Option<String>,
) -> (Vec<ServerConfig>, Vec<String>) {
    let mut servers: Vec<ServerConfig> = Vec::new();
    let mut problems = Vec::new();
    for file in files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                problems.push(format!("{}: {e}", file.display()));
                continue;
            }
        };
        let (found, mut p) = parse(&text, file, env);
        problems.append(&mut p);
        for s in found {
            servers.retain(|o| o.name != s.name);
            servers.push(s);
        }
    }
    (servers, problems)
}

/// The entries of one file's text.
pub fn parse(
    text: &str,
    file: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> (Vec<ServerConfig>, Vec<String>) {
    let at = file.display();
    let root: Value = match jsonc_parser::parse_to_serde_value(text, &Default::default()) {
        Ok(v) => v,
        Err(e) => return (vec![], vec![format!("{at}: {e}")]),
    };
    let Some(entries) = root.get("mcpServers") else {
        return (vec![], vec![format!("{at}: no \"mcpServers\" object")]);
    };
    let Some(entries) = entries.as_object() else {
        return (
            vec![],
            vec![format!("{at}: \"mcpServers\" is not an object")],
        );
    };
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for (name, entry) in entries {
        match entry_of(name, entry, file, env) {
            Ok(s) => out.push(s),
            Err(e) => problems.push(format!("{at}: server '{name}': {e}; skipped")),
        }
    }
    (out, problems)
}

fn entry_of(
    name: &str,
    v: &Value,
    file: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ServerConfig, String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("names may contain only letters, digits, '_' and '-'".into());
    }
    let o = v.as_object().ok_or("not an object")?;
    let s = |k: &str| o.get(k).and_then(Value::as_str);
    let list = |k: &str| -> Result<Option<Vec<String>>, String> {
        match o.get(k) {
            None => Ok(None),
            Some(Value::Array(a)) => a
                .iter()
                .map(|x| {
                    x.as_str()
                        .map(str::to_owned)
                        .ok_or(format!("\"{k}\" must list strings"))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(format!("\"{k}\" must be a list")),
        }
    };
    let pairs = |k: &str| -> Result<Vec<(String, String)>, String> {
        match o.get(k) {
            None => Ok(vec![]),
            Some(Value::Object(m)) => m
                .iter()
                .map(|(key, val)| {
                    let val = val
                        .as_str()
                        .ok_or(format!("\"{k}\" values must be strings"))?;
                    Ok((key.clone(), expand(val, env)?))
                })
                .collect(),
            Some(_) => Err(format!("\"{k}\" must be an object")),
        }
    };
    let kind = s("type");
    let transport = match (s("command"), s("url"), kind) {
        (_, _, Some("sse")) => {
            return Err("the SSE transport isn't supported; servers that offer it usually also serve streamable HTTP, often at /mcp".into());
        }
        (Some(command), None, None | Some("stdio")) => Transport::Stdio {
            command: expand(command, env)?,
            args: list("args")?
                .unwrap_or_default()
                .iter()
                .map(|a| expand(a, env))
                .collect::<Result<_, _>>()?,
            env: pairs("env")?,
            cwd: s("cwd").map(|c| {
                let c = PathBuf::from(c);
                match file.parent() {
                    Some(dir) if c.is_relative() => dir.join(c),
                    _ => c,
                }
            }),
        },
        (None, Some(url), None | Some("http" | "streamable-http")) => Transport::Http {
            url: expand(url, env)?,
            headers: pairs("headers")?,
        },
        (Some(_), Some(_), _) => return Err("give \"command\" or \"url\", not both".into()),
        (None, None, _) => return Err("needs \"command\" (stdio) or \"url\" (HTTP)".into()),
        (_, _, Some(t)) => return Err(format!("unknown type \"{t}\" for this entry")),
    };
    let (approval, tool_approval) = match o.get("approval") {
        None => (Approval::Auto, BTreeMap::new()),
        Some(Value::String(a)) => (
            Approval::parse(a)
                .ok_or(format!("unknown approval \"{a}\": use auto, ask or never"))?,
            BTreeMap::new(),
        ),
        Some(Value::Object(m)) => {
            let per_tool = tool_approvals(m)?;
            (
                per_tool.get("*").copied().unwrap_or(Approval::Auto),
                per_tool,
            )
        }
        Some(_) => return Err("\"approval\" must be a string or an object".into()),
    };
    let timeout = match o.get("timeout") {
        None => DEFAULT_TIMEOUT,
        Some(t) => t
            .as_u64()
            .filter(|t| *t > 0)
            .ok_or("\"timeout\" must be a whole number of seconds")?,
    };
    Ok(ServerConfig {
        name: name.to_owned(),
        transport,
        enabled: o.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        tools: list("tools")?,
        disabled_tools: list("disabledTools")?.unwrap_or_default(),
        approval,
        tool_approval,
        timeout: Duration::from_secs(timeout),
        source: file.to_owned(),
    })
}

/// `{"tool": "never", ...}`; the key `*` sets the server's default.
fn tool_approvals(m: &Map<String, Value>) -> Result<BTreeMap<String, Approval>, String> {
    m.iter()
        .map(|(k, v)| {
            let a = v
                .as_str()
                .and_then(Approval::parse)
                .ok_or(format!("approval for \"{k}\" must be auto, ask or never"))?;
            Ok((k.clone(), a))
        })
        .collect()
}

/// `${VAR}` and `${VAR:-default}` from the environment; an unset variable
/// without a default is an error, so a missing token doesn't go out empty.
pub fn expand(s: &str, env: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| format!("unclosed \"${{\" in \"{s}\""))?;
        let inner = &after[..end];
        let (var, default) = match inner.split_once(":-") {
            Some((v, d)) => (v, Some(d)),
            None => (inner, None),
        };
        match (env(var), default) {
            (Some(v), _) => out.push_str(&v),
            (None, Some(d)) => out.push_str(d),
            (None, None) => return Err(format!("${{{var}}} is not set")),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(k: &str) -> Option<String> {
        (k == "TOKEN").then(|| "t0k".to_owned())
    }

    fn one(text: &str) -> Result<ServerConfig, Vec<String>> {
        let (mut s, p) = parse(text, Path::new("/p/.psi/mcp.json"), &env);
        if p.is_empty() {
            Ok(s.remove(0))
        } else {
            Err(p)
        }
    }

    #[test]
    fn stdio_and_http_entries_in_the_shared_format() {
        let s = one(r#"{"mcpServers": {"fs": {"command": "npx", "args": ["-y", "srv", "."], "env": {"K": "${TOKEN}"}, "cwd": "sub"}}}"#).unwrap();
        assert_eq!(
            s.transport,
            Transport::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), "srv".into(), ".".into()],
                env: vec![("K".into(), "t0k".into())],
                cwd: Some("/p/.psi/sub".into()),
            }
        );
        assert!(s.enabled && s.approval == Approval::Auto);
        assert_eq!(s.timeout, Duration::from_secs(60));
        let h = one(r#"{"mcpServers": {"docs": {"url": "https://x/mcp", "headers": {"Authorization": "Bearer ${TOKEN}"}, "timeout": 5}}}"#).unwrap();
        assert_eq!(
            h.transport,
            Transport::Http {
                url: "https://x/mcp".into(),
                headers: vec![("Authorization".into(), "Bearer t0k".into())],
            }
        );
        assert_eq!(h.timeout, Duration::from_secs(5));
    }

    #[test]
    fn psi_fields_filter_and_approve() {
        let s = one(r#"{"mcpServers": {"gh": {"command": "gh-mcp", "tools": ["a", "b"], "disabledTools": ["b"], "approval": {"*": "ask", "a": "never"}}}}"#).unwrap();
        assert!(s.offers("a") && !s.offers("b") && !s.offers("c"));
        assert_eq!(s.approval_for("a"), Approval::Never);
        assert_eq!(s.approval_for("c"), Approval::Ask);
    }

    #[test]
    fn bad_entries_are_skipped_with_the_reason() {
        let text = r#"{"mcpServers": {
            "ok": {"command": "x"},
            "old": {"type": "sse", "url": "http://x/sse"},
            "none": {},
            "bad name": {"command": "x"},
            "unset": {"command": "x", "env": {"K": "${NOPE}"}},
            "def": {"command": "x", "env": {"K": "${NOPE:-d}"}}
        }}"#;
        let (s, p) = parse(text, Path::new("m.json"), &env);
        let names: Vec<&str> = s.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["ok", "def"]);
        assert_eq!(p.len(), 4, "{p:?}");
        assert!(
            p[0].contains("'old': the SSE transport isn't supported"),
            "{p:?}"
        );
        assert!(p[3].contains("${NOPE} is not set"), "{p:?}");
    }

    #[test]
    fn a_later_file_replaces_an_entry_of_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.json");
        let proj = dir.path().join("proj.json");
        std::fs::write(
            &user,
            r#"{"mcpServers": {"a": {"command": "u"}, "b": {"command": "u"}}}"#,
        )
        .unwrap();
        std::fs::write(&proj, r#"{"mcpServers": {"a": {"command": "p"}}}"#).unwrap();
        let missing = dir.path().join("nope.json");
        let (s, p) = load(&[user, proj, missing], &env);
        assert!(p.is_empty(), "{p:?}");
        let got: Vec<(&str, &Transport)> =
            s.iter().map(|s| (s.name.as_str(), &s.transport)).collect();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "b");
        assert!(matches!(got[1].1, Transport::Stdio { command, .. } if command == "p"));
    }
}
