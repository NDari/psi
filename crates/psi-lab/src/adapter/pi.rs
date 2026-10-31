//! pi's `--mode json` event stream to session entries.
//!
//! Only finished messages (`message_end`) become entries; streaming updates
//! and tool execution events are dropped. pi's system prompt arrives as
//! named sections, kept as a `custom` entry since its exact joined text is
//! pi's business.

use std::io::BufRead;

use psi_session::{Block, Entry, EntryBody, IdGen, Role, StopReason, Usage};
use serde_json::{Value, json};

use crate::Result;
use crate::error::IoContext;

/// A converted pi run.
#[derive(Debug, Clone, Default)]
pub struct PiRun {
    pub entries: Vec<Entry>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    /// pi emitted `agent_settled`: the run ended in an orderly way.
    pub settled: bool,
    pub aborted: bool,
    /// The last failed model request's error text.
    pub last_error: Option<String>,
    /// Lines that were not JSON (for example cut off by a kill).
    pub bad_lines: usize,
}

pub fn convert(r: impl BufRead) -> Result<PiRun> {
    let mut run = PiRun::default();
    let mut ids = IdGen::new();
    for line in r.lines() {
        let line = line.ctx(|| "reading pi's event stream".into())?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<Value>(&line) else {
            run.bad_lines += 1;
            continue;
        };
        match ev["type"].as_str() {
            Some("session") => {
                run.session_id = ev["id"].as_str().map(str::to_owned);
                run.cwd = ev["cwd"].as_str().map(str::to_owned);
                let data = json!({"agent": "pi", "session_id": ev["id"],
                                  "cwd": ev["cwd"], "version": ev["version"]});
                run.entries.push(ids.push(
                    0,
                    EntryBody::Custom {
                        custom_type: "session_info".into(),
                        data,
                    },
                ));
            }
            Some("message_end") => {
                if let Some(body) = message(&ev["message"], &mut run) {
                    let ts = ev["message"]["timestamp"].as_u64().unwrap_or(0);
                    run.entries.push(ids.push(ts, body));
                }
            }
            Some("agent_settled") => {
                run.settled = true;
                run.aborted = ev["aborted"].as_bool().unwrap_or(false);
            }
            _ => {}
        }
    }
    // The session header has no millisecond time; borrow the first message's.
    let first_ts = run.entries.iter().map(|e| e.ts).find(|&t| t > 0);
    if let (Some(ts), Some(first)) = (first_ts, run.entries.first_mut())
        && first.ts == 0
    {
        first.ts = ts;
    }
    Ok(run)
}

fn message(m: &Value, run: &mut PiRun) -> Option<EntryBody> {
    match m["role"].as_str()? {
        "system" => Some(EntryBody::Custom {
            custom_type: "system_prompt".into(),
            data: json!({"sections": m["sections"], "tools_added": m["toolsAdded"]}),
        }),
        "user" => Some(EntryBody::Message {
            role: Role::User,
            content: blocks(&m["content"]),
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        }),
        "assistant" => {
            let stop_reason = match m["stopReason"].as_str() {
                Some("stop") => Some(StopReason::Stop),
                Some("toolUse") => Some(StopReason::ToolUse),
                Some("length") => Some(StopReason::Length),
                Some("error") => Some(StopReason::Error),
                Some("aborted") => Some(StopReason::Aborted),
                _ => None,
            };
            let error = m["errorMessage"].as_str().map(str::to_owned);
            if stop_reason == Some(StopReason::Error) {
                run.last_error = Some(error.clone().unwrap_or_else(|| "error".into()));
            }
            let u = &m["usage"];
            let usage = u.is_object().then(|| Usage {
                input: u["input"].as_u64().unwrap_or(0),
                output: u["output"].as_u64().unwrap_or(0),
                cache_read: u["cacheRead"].as_u64().unwrap_or(0),
                cache_write: u["cacheWrite"].as_u64().unwrap_or(0),
            });
            Some(EntryBody::Message {
                role: Role::Assistant,
                content: blocks(&m["content"]),
                usage,
                stop_reason,
                error,
                model: m["model"].as_str().map(str::to_owned),
            })
        }
        "toolResult" => Some(EntryBody::ToolResult {
            tool_call_id: m["toolCallId"].as_str().unwrap_or_default().to_owned(),
            tool_name: m["toolName"].as_str().unwrap_or_default().to_owned(),
            content: blocks(&m["content"]),
            is_error: m["isError"].as_bool().unwrap_or(false),
        }),
        _ => None,
    }
}

fn blocks(content: &Value) -> Vec<Block> {
    if let Some(s) = content.as_str() {
        return vec![Block::Text { text: s.to_owned() }];
    }
    let Some(list) = content.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|b| match b["type"].as_str()? {
            "text" => Some(Block::Text {
                text: b["text"].as_str().unwrap_or_default().to_owned(),
            }),
            "thinking" => Some(Block::Thinking {
                thinking: b["thinking"].as_str().unwrap_or_default().to_owned(),
            }),
            "toolCall" => Some(Block::ToolCall {
                id: b["id"].as_str().unwrap_or_default().to_owned(),
                name: b["name"].as_str().unwrap_or_default().to_owned(),
                arguments: b["arguments"].clone(),
                raw: None,
            }),
            "image" => Some(Block::Text {
                text: format!("[image {}]", b["mimeType"].as_str().unwrap_or("?")),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real pi 1.4.0 trial (fix_env_parser, Qwen3.5 4B): the
    /// first two turns, a failed edit, and the step cap's aborted request.
    const FIXTURE: &str = include_str!("../../tests/fixtures/pi-run.jsonl");

    #[test]
    fn converts_a_real_pi_run() {
        let run = convert(FIXTURE.as_bytes()).unwrap();
        assert_eq!(
            run.session_id.as_deref(),
            Some("01a1883f-3e4a-7011-bb7a-8c3f19dcbc82")
        );
        assert_eq!(run.cwd.as_deref(), Some("/work/project"));
        assert!(run.settled);
        assert!(run.aborted);
        assert_eq!(run.bad_lines, 0);
        assert_eq!(
            run.last_error.as_deref(),
            Some("This operation was aborted")
        );

        let kinds: Vec<&str> = run
            .entries
            .iter()
            .map(|e| match &e.body {
                EntryBody::Custom { custom_type, .. } => custom_type.as_str(),
                EntryBody::Message {
                    role: Role::User, ..
                } => "user",
                EntryBody::Message {
                    role: Role::Assistant,
                    ..
                } => "assistant",
                EntryBody::ToolResult { .. } => "tool_result",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "session_info",
                "system_prompt",
                "user",
                "assistant",
                "tool_result",
                "tool_result",
                "assistant",
                "tool_result",
                "assistant",
            ]
        );
        // Entries chain into one branch and the header borrows a real time.
        for w in run.entries.windows(2) {
            assert_eq!(w[1].parent_id.as_deref(), Some(w[0].id.as_str()));
        }
        assert!(run.entries[0].ts > 0);

        let first = &run.entries[3];
        let calls: Vec<_> = first.tool_calls().map(|(_, n, a)| (n, a.clone())).collect();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "read");
        assert_eq!(calls[0].1["path"], ".env.example");
        let EntryBody::Message {
            usage, stop_reason, ..
        } = &first.body
        else {
            panic!("not a message");
        };
        assert_eq!(*stop_reason, Some(StopReason::ToolUse));
        assert_eq!(usage.unwrap().input, 1692);

        let EntryBody::ToolResult {
            is_error,
            tool_name,
            ..
        } = &run.entries[7].body
        else {
            panic!("not a tool result");
        };
        assert!(*is_error);
        assert_eq!(tool_name, "edit");

        // The step cap's aborted request has no content and is not a step.
        let last = run.entries.last().unwrap();
        assert!(!last.is_model_step());
        assert_eq!(run.entries.iter().filter(|e| e.is_model_step()).count(), 2);
    }

    #[test]
    fn non_json_lines_are_counted_not_fatal() {
        let run = convert("{\"type\":\"agent_start\"}\n{\"type\":\"mess".as_bytes()).unwrap();
        assert_eq!(run.bad_lines, 1);
        assert!(!run.settled);
    }

    #[test]
    fn string_user_content_becomes_text() {
        let line =
            r#"{"type":"message_end","message":{"role":"user","content":"hi","timestamp":7}}"#;
        let run = convert(line.as_bytes()).unwrap();
        let EntryBody::Message { content, .. } = &run.entries[0].body else {
            panic!("not a message");
        };
        assert_eq!(content, &vec![Block::Text { text: "hi".into() }]);
        assert_eq!(run.entries[0].ts, 7);
    }
}
