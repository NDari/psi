//! `psi inspect` (psi-plan.md §12.2): a session or trial as a timeline of
//! steps, with suspicious patterns flagged.

// TODO(phase-7): the `--html` report (§12.2) alongside HTML export.

use std::fmt::Write;
use std::path::Path;

use psi_session::{Block, Entry, EntryBody, Role, StopReason, Usage, read_entries, text_of};
use serde::Serialize;
use serde_json::Value;

use crate::error::{IoContext, other};
use crate::eval::TrialResult;
use crate::{Result, fsutil};

/// A session file, plus its trial result when it came from an eval.
#[derive(Debug, Clone)]
pub struct Record {
    pub entries: Vec<Entry>,
    pub result: Option<TrialResult>,
}

/// Loads `session.jsonl`, or a trial directory holding it and `result.json`.
pub fn load(path: &Path) -> Result<Record> {
    let (session, result_path) = if path.is_dir() {
        (path.join("session.jsonl"), Some(path.join("result.json")))
    } else {
        (path.to_owned(), None)
    };
    let file = std::fs::File::open(&session).ctx(|| format!("opening {}", session.display()))?;
    let entries = read_entries(std::io::BufReader::new(file))
        .map_err(|e| other(format!("{}: {e}", session.display())))?;
    let result = match result_path {
        Some(p) if p.is_file() => {
            let text = fsutil::read_to_string(&p)?;
            Some(serde_json::from_str(&text).map_err(|e| other(format!("{}: {e}", p.display())))?)
        }
        _ => None,
    };
    Ok(Record { entries, result })
}

#[derive(Debug, Clone, Serialize)]
pub struct Timeline {
    pub cwd: Option<String>,
    pub events: Vec<Event>,
    pub patterns: Vec<Pattern>,
    pub result: Option<TrialResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    User {
        text: String,
    },
    Step(Step),
    /// A model request that failed or was aborted before it produced anything.
    RequestFailed {
        error: String,
    },
    Guard {
        guard: String,
        message: String,
    },
    Policy {
        tool_call_id: String,
        decision: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub n: u32,
    pub text: String,
    pub calls: Vec<Call>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<StopReason>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub result: Option<String>,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pattern {
    pub kind: &'static str,
    pub step: Option<u32>,
    pub detail: String,
}

pub fn timeline(rec: &Record) -> Timeline {
    let mut cwd = None;
    let mut events = Vec::new();
    let mut n = 0;
    let mut step_cap_hit = rec.result.as_ref().is_some_and(|r| r.step_cap_hit);
    for e in &rec.entries {
        match &e.body {
            EntryBody::Custom { custom_type, data } => match custom_type.as_str() {
                "session_info" => cwd = data["cwd"].as_str().map(str::to_owned),
                "eval_end" => step_cap_hit |= data["step_cap_hit"] == Value::Bool(true),
                _ => {}
            },
            EntryBody::Message {
                role: Role::User,
                content,
                ..
            } => events.push(Event::User {
                text: text_of(content),
            }),
            EntryBody::Message {
                role: Role::Assistant,
                content,
                usage,
                stop_reason,
                error,
                ..
            } => {
                if !e.is_model_step() {
                    events.push(Event::RequestFailed {
                        error: error.clone().unwrap_or_else(|| "empty response".into()),
                    });
                    continue;
                }
                n += 1;
                let calls = content
                    .iter()
                    .filter_map(|b| match b {
                        Block::ToolCall {
                            id,
                            name,
                            arguments,
                            ..
                        } => Some(Call {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: arguments.clone(),
                            result: None,
                            is_error: false,
                        }),
                        _ => None,
                    })
                    .collect();
                events.push(Event::Step(Step {
                    n,
                    text: text_of(content),
                    calls,
                    usage: *usage,
                    stop_reason: *stop_reason,
                }));
            }
            EntryBody::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                let call = events.iter_mut().rev().find_map(|ev| match ev {
                    Event::Step(s) => s.calls.iter_mut().find(|c| &c.id == tool_call_id),
                    _ => None,
                });
                if let Some(c) = call {
                    c.result = Some(text_of(content));
                    c.is_error = *is_error;
                }
            }
            EntryBody::GuardEvent { guard, message, .. } => events.push(Event::Guard {
                guard: guard.clone(),
                message: message.clone(),
            }),
            EntryBody::PolicyDecision {
                tool_call_id,
                decision,
                reason,
                ..
            } => events.push(Event::Policy {
                tool_call_id: tool_call_id.clone(),
                decision: decision.clone(),
                reason: reason.clone(),
            }),
            _ => {}
        }
    }
    let mut patterns = detect(&events, cwd.as_deref());
    if step_cap_hit {
        patterns.push(Pattern {
            kind: "step_cap",
            step: None,
            detail: "the run hit the step cap".into(),
        });
    }
    Timeline {
        cwd,
        events,
        patterns,
        result: rec.result.clone(),
    }
}

/// Flags loops, unchanged retries, retries after a block or denial (H-04),
/// no-op edits, truncated calls, doubled working-directory paths, context
/// overflows and nested sessions.
fn detect(events: &[Event], cwd: Option<&str>) -> Vec<Pattern> {
    let mut out = Vec::new();
    let mut prev: Option<(&Call, u32)> = None;
    let mut run_len = 1;
    let mut last_step: Option<&Step> = None;
    // The tool of a call that was blocked or denied, until the next call.
    let mut blocked: Option<(&str, &str)> = None;
    for ev in events {
        match ev {
            Event::Policy {
                tool_call_id,
                decision,
                ..
            } if matches!(decision.as_str(), "blocked" | "denied" | "refused") => {
                if let Some(c) =
                    last_step.and_then(|s| s.calls.iter().find(|c| &c.id == tool_call_id))
                {
                    blocked = Some((c.name.as_str(), decision.as_str()));
                }
            }
            Event::RequestFailed { error } if is_overflow(error) => out.push(Pattern {
                kind: "context_overflow",
                step: None,
                detail: error.clone(),
            }),
            Event::Step(s) => {
                last_step = Some(s);
                if let (Some((tool, how)), Some(c)) = (blocked.take(), s.calls.first())
                    && c.name == tool
                {
                    out.push(Pattern {
                        kind: "retry_after_block",
                        step: Some(s.n),
                        detail: format!("{tool} called again right after a call was {how}"),
                    });
                }
                if s.stop_reason == Some(StopReason::Length) {
                    out.push(Pattern {
                        kind: "truncated",
                        step: Some(s.n),
                        detail: "the response hit the output limit".into(),
                    });
                }
                for c in &s.calls {
                    let same =
                        prev.is_some_and(|(p, _)| p.name == c.name && p.arguments == c.arguments);
                    if same {
                        run_len += 1;
                        if run_len == 3 {
                            out.push(Pattern {
                                kind: "loop",
                                step: Some(s.n),
                                detail: format!(
                                    "{} called 3 times in a row with the same arguments",
                                    c.name
                                ),
                            });
                        }
                        if let Some((p, _)) = prev
                            && p.is_error
                        {
                            out.push(Pattern {
                                kind: "retry_unchanged",
                                step: Some(s.n),
                                detail: format!("{} repeated unchanged after it failed", c.name),
                            });
                        }
                    } else {
                        run_len = 1;
                    }
                    if is_noop_edit(c) {
                        out.push(Pattern {
                            kind: "noop_edit",
                            step: Some(s.n),
                            detail: "an edit whose old and new text are the same".into(),
                        });
                    }
                    if let Some(p) = doubled_path(c, cwd) {
                        out.push(Pattern {
                            kind: "doubled_cwd_path",
                            step: Some(s.n),
                            detail: format!("{} path {p:?} repeats the working directory", c.name),
                        });
                    }
                    if c.name == "bash"
                        && let Some(cmd) = c.arguments["command"].as_str()
                        && is_nested_session(cmd)
                    {
                        out.push(Pattern {
                            kind: "nested_session",
                            step: Some(s.n),
                            detail: format!("bash runs an agent: {cmd}"),
                        });
                    }
                    prev = Some((c, s.n));
                }
            }
            _ => {}
        }
    }
    out
}

fn is_overflow(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    e.contains("exceed_context_size") || e.contains("context size") || e.contains("context length")
}

fn is_noop_edit(c: &Call) -> bool {
    if c.name != "edit" {
        return false;
    }
    let a = &c.arguments;
    let same = |o: &Value, n: &Value| o.is_string() && o == n;
    // Psi's shape, then pi's single and multi-edit shapes.
    same(&a["old_text"], &a["new_text"])
        || same(&a["oldText"], &a["newText"])
        || a["edits"].as_array().is_some_and(|es| {
            !es.is_empty() && es.iter().all(|e| same(&e["oldText"], &e["newText"]))
        })
}

/// A relative path that starts with the tail of the working directory,
/// like `project/x` or `work/project/x` from `/work/project`.
fn doubled_path(c: &Call, cwd: Option<&str>) -> Option<String> {
    let cwd = cwd?;
    let path = c.arguments["path"]
        .as_str()
        .or_else(|| c.arguments["file_path"].as_str())?;
    if path.starts_with('/') || path.starts_with('~') {
        return None;
    }
    let parts: Vec<&str> = cwd.split('/').filter(|p| !p.is_empty()).collect();
    (0..parts.len())
        .map(|i| parts[i..].join("/"))
        .any(|tail| path == tail || path.starts_with(&format!("{tail}/")))
        .then(|| path.to_owned())
}

/// Whether a bash command starts another agent run (`psi -p`, `pi --mode json`, ...).
fn is_nested_session(cmd: &str) -> bool {
    let words: Vec<&str> = cmd
        .split(|ch: char| ch.is_whitespace() || ";&|()`".contains(ch))
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().enumerate().any(|(i, w)| {
        let prog = w.rsplit('/').next().unwrap_or(w);
        (prog == "psi" || prog == "pi")
            && words[i + 1..]
                .iter()
                .take_while(|a| a.starts_with('-') || a.starts_with('"') || a.starts_with('\''))
                .any(|a| matches!(*a, "-p" | "--print" | "--json" | "--mode"))
    })
}

/// What to show.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Only steps with a failed call, and failed requests.
    pub failures: bool,
    pub guards: bool,
    pub blocked: bool,
    /// Only this step, in full.
    pub step: Option<u32>,
}

impl Filter {
    fn keeps(&self, ev: &Event) -> bool {
        if let Some(n) = self.step {
            return matches!(ev, Event::Step(s) if s.n == n);
        }
        if !(self.failures || self.guards || self.blocked) {
            return true;
        }
        (self.failures
            && match ev {
                Event::Step(s) => s.calls.iter().any(|c| c.is_error),
                Event::RequestFailed { .. } => true,
                _ => false,
            })
            || (self.guards && matches!(ev, Event::Guard { .. }))
            || (self.blocked && matches!(ev, Event::Policy { decision, .. } if decision != "allow"))
    }

    pub fn apply(&self, t: &Timeline) -> Timeline {
        let mut t = t.clone();
        t.events.retain(|e| self.keeps(e));
        if let Some(n) = self.step {
            t.patterns.retain(|p| p.step == Some(n));
        }
        t
    }
}

/// The terminal view. One line per item, cut to `width`, unless `full`.
pub fn render_text(t: &Timeline, full: bool) -> String {
    let width = if full { usize::MAX } else { 160 };
    let mut s = String::new();
    if let Some(r) = &t.result {
        let _ = writeln!(
            s,
            "{} {}: {} checks {}/{}, {} steps, {} tool errors{}",
            r.agent,
            r.task,
            r.outcome.as_str(),
            r.checks_passed,
            r.checks_total,
            r.steps,
            r.tool_errors,
            if r.reason.is_empty() {
                String::new()
            } else {
                format!("; {}", r.reason)
            }
        );
    }
    if let Some(c) = &t.cwd {
        let _ = writeln!(s, "cwd {c}");
    }
    for ev in &t.events {
        match ev {
            Event::User { text } => {
                let _ = writeln!(s, "\nuser: {}", cut(text, width));
            }
            Event::Step(st) => {
                let u = st
                    .usage
                    .map(|u| format!("  in {} out {}", u.prompt_total(), u.output))
                    .unwrap_or_default();
                let _ = writeln!(s, "\nstep {}{u}", st.n);
                if !st.text.trim().is_empty() {
                    let _ = writeln!(s, "  text: {}", cut(&st.text, width));
                }
                for c in &st.calls {
                    let args = serde_json::to_string(&c.arguments).unwrap_or_default();
                    let _ = writeln!(s, "  {} {}", c.name, cut(&args, width));
                    match &c.result {
                        Some(r) => {
                            let tag = if c.is_error { "ERROR" } else { "ok" };
                            let _ = writeln!(s, "    {tag}: {}", cut(r, width));
                        }
                        None => {
                            let _ = writeln!(s, "    (no result)");
                        }
                    }
                }
            }
            Event::RequestFailed { error } => {
                let _ = writeln!(s, "\nrequest failed: {}", cut(error, width));
            }
            Event::Guard { guard, message } => {
                let _ = writeln!(s, "\nguard {guard}: {}", cut(message, width));
            }
            Event::Policy {
                decision,
                reason,
                tool_call_id,
            } => {
                let _ = writeln!(
                    s,
                    "\npolicy {decision} ({tool_call_id}): {}",
                    cut(reason, width)
                );
            }
        }
    }
    if !t.patterns.is_empty() {
        s.push_str("\npatterns:\n");
        for p in &t.patterns {
            let at = p.step.map(|n| format!(" (step {n})")).unwrap_or_default();
            let _ = writeln!(s, "  - {}{at}: {}", p.kind, cut(&p.detail, width));
        }
    }
    s
}

/// One line: newlines shown as `\n`, cut at `width` characters.
fn cut(text: &str, width: usize) -> String {
    if width == usize::MAX {
        return text.to_owned();
    }
    let one = text.replace('\n', "\\n");
    let count = one.chars().count();
    if count <= width {
        return one;
    }
    let kept: String = one.chars().take(width).collect();
    format!("{kept}... ({count} chars)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::pi::convert;
    use serde_json::json;

    const FIXTURE: &str = include_str!("../tests/fixtures/pi-run.jsonl");

    fn call(name: &str, args: Value, is_error: bool) -> Call {
        Call {
            id: "c".into(),
            name: name.into(),
            arguments: args,
            result: Some(String::new()),
            is_error,
        }
    }

    fn step(n: u32, calls: Vec<Call>) -> Event {
        Event::Step(Step {
            n,
            text: String::new(),
            calls,
            usage: None,
            stop_reason: Some(StopReason::ToolUse),
        })
    }

    #[test]
    fn timeline_of_a_real_pi_run() {
        let run = convert(FIXTURE.as_bytes()).unwrap();
        let t = timeline(&Record {
            entries: run.entries,
            result: None,
        });
        assert_eq!(t.cwd.as_deref(), Some("/work/project"));
        let steps: Vec<&Step> = t
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Step(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].calls.len(), 2);
        assert!(
            steps[0]
                .calls
                .iter()
                .all(|c| c.result.is_some() && !c.is_error)
        );
        assert!(steps[1].calls[0].is_error);
        assert!(matches!(t.events.last(), Some(Event::RequestFailed { .. })));

        let failures = Filter {
            failures: true,
            ..Filter::default()
        }
        .apply(&t);
        assert_eq!(
            failures.events.len(),
            2,
            "the failed edit and the aborted request"
        );

        let text = render_text(&t, false);
        assert!(
            text.contains("\nstep 1  in 1692 out 51\n  read {\"path\":\".env.example\"}\n    ok: "),
            "{text}"
        );
        assert!(
            text.contains("    ERROR: Could not find edits[1] in envfile.py."),
            "{text}"
        );
        assert!(
            text.contains("request failed: This operation was aborted"),
            "{text}"
        );
    }

    #[test]
    fn step_filter_shows_one_step() {
        let run = convert(FIXTURE.as_bytes()).unwrap();
        let t = timeline(&Record {
            entries: run.entries,
            result: None,
        });
        let one = Filter {
            step: Some(2),
            ..Filter::default()
        }
        .apply(&t);
        assert_eq!(one.events.len(), 1);
        assert!(matches!(&one.events[0], Event::Step(s) if s.n == 2));
    }

    #[test]
    fn detects_loops_and_unchanged_retries() {
        let a = json!({"command": "pytest"});
        let ev = vec![
            step(1, vec![call("bash", a.clone(), true)]),
            step(2, vec![call("bash", a.clone(), true)]),
            step(3, vec![call("bash", a.clone(), false)]),
        ];
        let kinds: Vec<_> = detect(&ev, None)
            .into_iter()
            .map(|p| (p.kind, p.step))
            .collect();
        assert_eq!(
            kinds,
            [
                ("retry_unchanged", Some(2)),
                ("loop", Some(3)),
                ("retry_unchanged", Some(3))
            ]
        );
    }

    #[test]
    fn detects_retries_after_a_block() {
        let policy = |d: &str| Event::Policy {
            tool_call_id: "c".into(),
            decision: d.into(),
            reason: "r".into(),
        };
        let ev = vec![
            step(
                1,
                vec![call("bash", json!({"command": "touch /etc/x"}), true)],
            ),
            policy("blocked"),
            step(
                2,
                vec![call("bash", json!({"command": "sudo touch /etc/x"}), true)],
            ),
            policy("denied"),
            step(3, vec![call("read", json!({"path": "a"}), false)]),
        ];
        let kinds: Vec<_> = detect(&ev, None)
            .into_iter()
            .map(|p| (p.kind, p.step))
            .collect();
        assert_eq!(kinds, [("retry_after_block", Some(2))]);
    }

    #[test]
    fn detects_noop_edits_in_both_shapes() {
        assert!(is_noop_edit(&call(
            "edit",
            json!({"old_text": "a", "new_text": "a"}),
            false
        )));
        assert!(is_noop_edit(&call(
            "edit",
            json!({"edits": [{"oldText": "a", "newText": "a"}]}),
            false
        )));
        assert!(!is_noop_edit(&call(
            "edit",
            json!({"edits": [{"oldText": "a", "newText": "b"}]}),
            false
        )));
        assert!(!is_noop_edit(&call("edit", json!({}), false)));
    }

    #[test]
    fn detects_doubled_working_directory_paths() {
        let cwd = Some("/work/project");
        let c = |p: &str| call("read", json!({"path": p}), false);
        assert!(doubled_path(&c("work/project/x.py"), cwd).is_some());
        assert!(doubled_path(&c("project/x.py"), cwd).is_some());
        assert!(doubled_path(&c("x.py"), cwd).is_none());
        assert!(doubled_path(&c("/work/project/x.py"), cwd).is_none());
        assert!(doubled_path(&c("projects/x.py"), cwd).is_none());
    }

    #[test]
    fn detects_nested_sessions() {
        assert!(is_nested_session("psi -p 'fix it'"));
        assert!(is_nested_session(
            "cd x && /usr/local/bin/pi --mode json -p hi"
        ));
        assert!(!is_nested_session("grep -p pi file"));
        assert!(!is_nested_session("python3 pi.py -p"));
    }

    #[test]
    fn cut_keeps_one_line() {
        assert_eq!(cut("a\nb", 10), "a\\nb");
        assert_eq!(cut("abcdef", 3), "abc... (6 chars)");
        assert_eq!(cut("a\nb", usize::MAX), "a\nb");
    }
}
