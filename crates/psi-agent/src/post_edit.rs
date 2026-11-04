//! The post-edit check (psi-plan.md §10, D-36): after a turn whose write or
//! edit calls changed files, run the project's check command once and add a
//! short result to the last of those calls.

use psi_tools::ToolOutput;
use serde_json::Value;

/// Output lines of a failed check the model sees; errors usually come first.
pub const MAX_LINES: usize = 20;
/// A longer line is cut in the middle: test libraries print whole values on
/// one line (testify's expected and actual made two results of 16K
/// characters in the cog trial, 2026-11-03).
const MAX_LINE_CHARS: usize = 200;
/// The kept lines together, at most.
const MAX_CHARS: usize = 3000;
const DEFAULT_TIMEOUT_S: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostEditCheck {
    pub command: String,
    pub timeout_s: u64,
}

impl PostEditCheck {
    /// From the `post_edit_check` setting: a command, or
    /// `{ "command": ..., "timeout": seconds }`. Null or empty is off.
    pub fn from_setting(v: &Value) -> Result<Option<Self>, String> {
        let (command, timeout) = match v {
            Value::Null => return Ok(None),
            Value::String(s) => (s.as_str(), None),
            Value::Object(o) => (
                o.get("command").and_then(Value::as_str).unwrap_or(""),
                o.get("timeout").and_then(Value::as_u64),
            ),
            _ => {
                return Err(
                    "post_edit_check is a command, or {\"command\": ..., \"timeout\": seconds}"
                        .into(),
                );
            }
        };
        if command.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            command: command.to_owned(),
            timeout_s: timeout.unwrap_or(DEFAULT_TIMEOUT_S),
        }))
    }
}

/// Longer commands are cut in the result: it repeats after every turn with edits.
const SHOWN_COMMAND: usize = 60;

fn shown(command: &str) -> String {
    let one_line = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= SHOWN_COMMAND {
        return one_line;
    }
    let cut: String = one_line.chars().take(SHOWN_COMMAND - 3).collect();
    format!("{cut}...")
}

/// What the model sees after the edit's own result.
pub fn summary(command: &str, out: &ToolOutput) -> String {
    let command = &shown(command);
    if !out.is_error {
        return format!("[post-edit check passed: `{command}`]");
    }
    let text = out.text.trim_end();
    // bash ends a failure with a status line such as "[exit code 1]".
    let (body, status) = match text.rsplit_once('\n') {
        Some((b, last)) if last.starts_with('[') => (b, Some(last)),
        _ if text.starts_with('[') && !text.contains('\n') => ("", Some(text)),
        _ => (text, None),
    };
    let lines: Vec<&str> = body.lines().collect();
    let mut s = format!("[post-edit check failed: `{command}`]");
    let (mut kept, mut chars) = (0, 0);
    for l in lines.iter().take(MAX_LINES) {
        let l = cut_line(l);
        if kept > 0 && chars + l.chars().count() > MAX_CHARS {
            break;
        }
        chars += l.chars().count();
        kept += 1;
        s.push('\n');
        s.push_str(&l);
    }
    if lines.len() > kept {
        s.push_str(&format!(
            "\n[... {} more lines; run the command to see them]",
            lines.len() - kept
        ));
    }
    if let Some(st) = status {
        s.push('\n');
        s.push_str(st);
    }
    s
}

/// `line`, or its start and end around a note when it is over [`MAX_LINE_CHARS`].
fn cut_line(line: &str) -> String {
    let n = line.chars().count();
    if n <= MAX_LINE_CHARS {
        return line.to_owned();
    }
    let half = MAX_LINE_CHARS / 2;
    let head: String = line.chars().take(half).collect();
    let tail: String = line.chars().skip(n - half).collect();
    format!("{head} [... {} characters cut ...] {tail}", n - 2 * half)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn setting_forms() {
        assert_eq!(PostEditCheck::from_setting(&Value::Null).unwrap(), None);
        assert_eq!(PostEditCheck::from_setting(&json!("  ")).unwrap(), None);
        let c = PostEditCheck::from_setting(&json!("cargo check -q"))
            .unwrap()
            .unwrap();
        assert_eq!((c.command.as_str(), c.timeout_s), ("cargo check -q", 120));
        let c = PostEditCheck::from_setting(&json!({"command": "make", "timeout": 30}))
            .unwrap()
            .unwrap();
        assert_eq!((c.command.as_str(), c.timeout_s), ("make", 30));
        assert!(PostEditCheck::from_setting(&json!(3)).is_err());
    }

    #[test]
    fn pass_is_one_line() {
        let s = summary("make", &ToolOutput::ok("lots\nof\noutput"));
        assert_eq!(s, "[post-edit check passed: `make`]");
    }

    #[test]
    fn failure_keeps_first_lines_and_status() {
        let body: Vec<String> = (1..=25).map(|i| format!("error {i}")).collect();
        let out = ToolOutput::err(format!("{}\n[exit code 2]", body.join("\n")));
        let s = summary("make", &out);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "[post-edit check failed: `make`]");
        assert_eq!(lines[1], "error 1");
        assert_eq!(lines[20], "error 20");
        assert_eq!(lines[21], "[... 5 more lines; run the command to see them]");
        assert_eq!(lines[22], "[exit code 2]");
        assert_eq!(lines.len(), 23);
    }

    #[test]
    fn long_lines_are_cut_and_the_whole_is_capped() {
        // testify's shape (the cog trial): whole values on one line each.
        let value = format!("\"| a | {} |\"", "x".repeat(6000));
        let body = [
            "--- FAIL: TestWordDocumentationCurrent (0.00s)".to_owned(),
            format!("        expected: {value}"),
            format!("        actual  : {value}"),
            "        -| ? | ( -- value ) |".to_owned(),
            "        +| ? | ( quote -- ) |".to_owned(),
        ];
        let out = ToolOutput::err(format!("{}\n[exit code 1]", body.join("\n")));
        let s = summary("go test ./...", &out);
        assert!(s.chars().count() < 1000, "{} chars", s.chars().count());
        assert!(s.contains("characters cut ...]"), "{s}");
        assert!(
            s.contains("-| ? | ( -- value ) |") && s.ends_with("[exit code 1]"),
            "{s}"
        );
        // Past the total, the rest is counted instead.
        let many: Vec<String> = (0..20)
            .map(|i| format!("{i} {}", "y".repeat(500)))
            .collect();
        let s = summary("t", &ToolOutput::err(many.join("\n")));
        assert!(s.chars().count() <= 3300, "{} chars", s.chars().count());
        assert!(
            s.contains("more lines; run the command to see them]"),
            "{s}"
        );
    }

    #[test]
    fn long_commands_are_cut() {
        let long = format!("if [ -d tests ]; then {} fi", "x ".repeat(60));
        let s = summary(&long, &ToolOutput::ok(""));
        assert_eq!(s.chars().count(), "[post-edit check passed: ``]".len() + 60);
        assert!(s.ends_with("...`]"), "{s}");
    }

    #[test]
    fn failure_without_output() {
        let s = summary("false", &ToolOutput::err("(no output)\n[exit code 1]"));
        assert_eq!(
            s,
            "[post-edit check failed: `false`]\n(no output)\n[exit code 1]"
        );
    }
}
