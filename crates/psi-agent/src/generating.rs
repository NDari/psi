//! What the model is writing that no one sees as it comes: thinking, and
//! the arguments of a tool call. A local model can spend minutes on either
//! (the Qwen3.8 27B thought for 236 s before one summary, and a 300-line
//! `write` call is about 100 s at its 33 tokens a second), and the TUI said
//! "waiting for the model" all that time. The agent reports how far each
//! has come, for a live line; the text itself goes into the session entry
//! when the response ends.

use std::sync::LazyLock;

use psi_ai::Delta;
use regex::Regex;

use crate::events::AgentEvent;

/// Characters between two reports of the same thing: about two a second
/// at 33 tokens a second, and few enough lines in `--json` output.
const STEP: u64 = 64;
/// How much of a call's arguments is kept to find its path in. `path`
/// comes first in the file tools' schemas (§4.4).
const HEAD: usize = 2048;

/// A whole `"path": "..."` in arguments still being written.
#[allow(clippy::expect_used)] // A constant pattern; tests cover it.
static PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""path"\s*:\s*("(?:[^"\\]|\\.)*")"#).expect("valid regex"));

#[derive(Debug, Default)]
struct Call {
    name: String,
    head: String,
    path: Option<String>,
    chars: u64,
    /// `chars` when it was last reported.
    sent: u64,
}

/// One response's progress. Make a new one for each request sent.
#[derive(Debug, Default)]
pub(crate) struct Generating {
    thinking: u64,
    thinking_sent: Option<u64>,
    calls: Vec<Call>,
}

impl Generating {
    /// The event to send for this delta, if it is time for one.
    pub fn on(&mut self, d: &Delta) -> Option<AgentEvent> {
        match d {
            Delta::Reasoning(t) => {
                self.thinking += t.chars().count() as u64;
                let due = self.thinking_sent.is_none_or(|s| self.thinking >= s + STEP);
                due.then(|| {
                    self.thinking_sent = Some(self.thinking);
                    AgentEvent::Thinking {
                        chars: self.thinking,
                    }
                })
            }
            // The name can come in pieces; each piece carries all of it so far.
            Delta::ToolCallStart { index, name } => {
                let call = self.call(*index);
                call.name.clone_from(name);
                Some(Self::event(*index, call))
            }
            Delta::ToolCallArgs { index, text } => {
                let call = self.call(*index);
                call.chars += text.chars().count() as u64;
                let mut found = false;
                if call.path.is_none() && call.head.len() < HEAD {
                    call.head.push_str(text);
                    call.path = PATH
                        .captures(&call.head)
                        .and_then(|c| serde_json::from_str(&c[1]).ok());
                    found = call.path.is_some();
                }
                (found || call.chars >= call.sent + STEP).then(|| Self::event(*index, call))
            }
            _ => None,
        }
    }

    fn call(&mut self, index: usize) -> &mut Call {
        if self.calls.len() <= index {
            self.calls.resize_with(index + 1, Call::default);
        }
        &mut self.calls[index]
    }

    fn event(index: usize, call: &mut Call) -> AgentEvent {
        call.sent = call.chars;
        AgentEvent::CallProgress {
            index,
            name: call.name.clone(),
            path: call.path.clone(),
            chars: call.chars,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(g: &mut Generating, text: &str) -> Option<AgentEvent> {
        g.on(&Delta::ToolCallArgs {
            index: 0,
            text: text.into(),
        })
    }

    fn progress(e: Option<AgentEvent>) -> Option<(String, Option<String>, u64)> {
        match e {
            Some(AgentEvent::CallProgress {
                name, path, chars, ..
            }) => Some((name, path, chars)),
            None => None,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn thinking_is_reported_at_once_and_then_every_step() {
        let mut g = Generating::default();
        let think = |g: &mut Generating, n: usize| match g.on(&Delta::Reasoning("x".repeat(n))) {
            Some(AgentEvent::Thinking { chars }) => Some(chars),
            None => None,
            other => panic!("{other:?}"),
        };
        assert_eq!(think(&mut g, 3), Some(3));
        assert_eq!(think(&mut g, 60), None);
        assert_eq!(think(&mut g, 4), Some(67));
        assert_eq!(think(&mut g, 63), None);
        // Counted in characters, not bytes.
        assert!(g.on(&Delta::Reasoning("é".into())).is_some());
        assert_eq!(g.thinking, 131);
        // Answer text is the screen's own business.
        assert!(g.on(&Delta::Text("hello".repeat(100))).is_none());
    }

    #[test]
    fn a_call_is_reported_with_its_name_then_its_path_then_its_size() {
        let mut g = Generating::default();
        let start = g.on(&Delta::ToolCallStart {
            index: 0,
            name: "write".into(),
        });
        assert_eq!(progress(start), Some(("write".into(), None, 0)));
        // The path as soon as its closing quote is there, escapes read.
        assert_eq!(progress(args(&mut g, r#"{"path":"src/a"#)), None);
        assert_eq!(
            progress(args(&mut g, r#" \"b\".py","text":"#)),
            Some(("write".into(), Some("src/a \"b\".py".into()), 32))
        );
        // Then every 64 characters.
        assert_eq!(progress(args(&mut g, &"x".repeat(63))), None);
        assert_eq!(
            progress(args(&mut g, "y")),
            Some(("write".into(), Some("src/a \"b\".py".into()), 96))
        );
    }

    #[test]
    fn a_call_without_a_path_reports_its_size_and_keeps_little() {
        let mut g = Generating::default();
        g.on(&Delta::ToolCallStart {
            index: 1,
            name: "bash".into(),
        });
        let piece = r#"{"command":"echo "#.to_owned() + &"a".repeat(1000);
        let mut last = None;
        for _ in 0..10 {
            last = g
                .on(&Delta::ToolCallArgs {
                    index: 1,
                    text: piece.clone(),
                })
                .or(last);
        }
        match last {
            Some(AgentEvent::CallProgress {
                index: 1,
                name,
                path: None,
                chars,
            }) => assert_eq!((name.as_str(), chars), ("bash", 10 * piece.len() as u64)),
            other => panic!("{other:?}"),
        }
        // Only the start is kept to look for a path in.
        assert!(g.calls[1].head.len() < HEAD + piece.len());
        // The call before it in the response was never named.
        assert_eq!(g.calls[0].chars, 0);
    }
}
