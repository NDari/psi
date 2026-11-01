//! Keeping the context in the window (psi-plan.md §7.4). First stage:
//! when the context passes the trigger, old tool results are replaced by a
//! one-line note that says what they were and how to get them back. No
//! model call. Summaries come later.

use std::collections::HashMap;

use psi_ai::ChatMessage;
use psi_session::{Entry, EntryBody};
use serde_json::Value;

/// D-20: estimate conservatively when the server's count is not known.
pub(crate) const CHARS_PER_TOKEN: u64 = 3;
/// The most recent tool results are never cleared (Anthropic keeps 3).
const KEEP_RECENT: usize = 3;
/// Results shorter than this stay: clearing them frees almost nothing.
const MIN_CLEAR_CHARS: usize = 800;
/// How much of a command or argument list a note repeats.
const NOTE_ARGS: usize = 80;

/// The context size (in tokens) at which old context is cleared: below
/// the room for one answer plus one tool result, and at most 70% of the
/// window. Never below a quarter of the window, so small windows don't
/// clear every turn.
pub(crate) fn trigger(n_ctx: u64, max_output: u64, max_result_tokens: u64) -> u64 {
    let reserve = max_output.max(4096) + max_result_tokens;
    (n_ctx * 7 / 10)
        .min(n_ctx.saturating_sub(reserve))
        .max(n_ctx / 4)
}

/// The size of the next request in tokens: the server's count for the last
/// response plus an estimate for what came after it, or an estimate for
/// all of it when there is no count, or when the history was cut since.
pub(crate) fn estimate_tokens(
    branch: &[&Entry],
    messages: &[ChatMessage],
    tools_chars: usize,
) -> u64 {
    let counted = branch
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, e)| match &e.body {
            EntryBody::Message { usage: Some(u), .. } if u.prompt_total() > 0 => {
                Some((i, u.prompt_total() + u.output))
            }
            _ => None,
        });
    if let Some((i, tokens)) = counted {
        let after = &branch[i + 1..];
        let cut = after.iter().any(|e| {
            matches!(
                e.body,
                EntryBody::Compaction { .. } | EntryBody::ClearedResults { .. }
            )
        });
        if !cut {
            let chars: usize = after.iter().map(|e| sent_chars(e)).sum();
            return tokens + chars as u64 / CHARS_PER_TOKEN;
        }
    }
    let chars: usize = messages.iter().map(message_chars).sum::<usize>() + tools_chars;
    chars as u64 / CHARS_PER_TOKEN
}

fn sent_chars(e: &Entry) -> usize {
    match &e.body {
        EntryBody::Message { content, .. }
        | EntryBody::ToolResult { content, .. }
        | EntryBody::CustomMessage { content, .. } => psi_session::text_of(content).len(),
        _ => 0,
    }
}

fn message_chars(m: &ChatMessage) -> usize {
    match m {
        ChatMessage::System(t) | ChatMessage::User(t) => t.len(),
        ChatMessage::Assistant { text, tool_calls } => {
            text.len()
                + tool_calls
                    .iter()
                    .map(|c| c.name.len() + c.arguments.len())
                    .sum::<usize>()
        }
        ChatMessage::Tool { content, .. } => content.len(),
    }
}

/// The old tool results to clear (their entry ids, from `sources`), oldest
/// first, so that about `need` tokens are freed, and the estimate of what
/// they free. `None` when they would free less than `min_batch`: each
/// clearing breaks the prompt cache from the first cleared result on, so it
/// is done in batches, never one small result at a time.
pub(crate) fn plan_clear(
    messages: &[ChatMessage],
    sources: &[Option<String>],
    need: u64,
    min_batch: u64,
) -> Option<(Vec<String>, u64)> {
    // (entry id, tool, arguments, content), in order. Call ids may repeat
    // across turns, so each result takes the latest call with its id.
    let mut calls: HashMap<&str, (&str, &str)> = HashMap::new();
    let mut results = Vec::new();
    for (m, src) in messages.iter().zip(sources) {
        match m {
            ChatMessage::Assistant { tool_calls, .. } => {
                for c in tool_calls {
                    calls.insert(&c.id, (&c.name, &c.arguments));
                }
            }
            ChatMessage::Tool { call_id, content } => {
                if let Some(id) = src {
                    let (name, args) = calls
                        .get(call_id.as_str())
                        .copied()
                        .unwrap_or(("tool", "{}"));
                    results.push((id, name, args, content));
                }
            }
            _ => {}
        }
    }
    let old = results.len().saturating_sub(KEEP_RECENT);
    let mut ids = Vec::new();
    let mut freed = 0;
    for (id, name, args, content) in &results[..old] {
        if freed >= need {
            break;
        }
        // A cleared result is a short note, so it is never cleared again.
        if content.len() < MIN_CLEAR_CHARS {
            continue;
        }
        let note = cleared_note(name, args, content);
        freed += (content.len() - note.len().min(content.len())) as u64 / CHARS_PER_TOKEN;
        ids.push((*id).clone());
    }
    (freed >= min_batch && !ids.is_empty()).then_some((ids, freed))
}

/// What a cleared result is sent as: the call that made it, its size, and
/// how to get it back.
pub(crate) fn cleared_note(name: &str, arguments: &str, content: &str) -> String {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let lines = content.lines().count();
    let size = if lines == 1 {
        "1 line".to_owned()
    } else {
        format!("{lines} lines")
    };
    let what = match (name, args["path"].as_str(), args["command"].as_str()) {
        ("read", Some(p), _) => match args["offset"].as_u64() {
            Some(o) if o > 1 => format!("read {p} from line {o}"),
            _ => format!("read {p}"),
        },
        ("bash", _, Some(c)) => format!("bash `{}`", cut(c)),
        _ => {
            let mut a = args.as_object().cloned().unwrap_or_default();
            a.remove("intent");
            format!("{name} {}", cut(&Value::Object(a).to_string()))
        }
    };
    format!("(Result cleared to save context: {what}, {size}. Run it again if you need it.)")
}

fn cut(s: &str) -> String {
    let s = s.lines().next().unwrap_or("");
    if s.chars().count() <= NOTE_ARGS {
        s.to_owned()
    } else {
        let kept: String = s.chars().take(NOTE_ARGS).collect();
        format!("{kept}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use psi_ai::RawToolCall;

    #[test]
    fn trigger_leaves_room_and_fires_in_small_windows() {
        // 64K, 8K answers, results up to 15% of the window: 70% wins.
        assert_eq!(trigger(65_536, 8192, 9830), 45_875);
        // 16K: the reserve wins.
        assert_eq!(trigger(16_384, 4096, 2457), 16_384 - 4096 - 2457);
        // A reserve larger than the window: a quarter of it.
        assert_eq!(trigger(8192, 8192, 1000), 2048);
    }

    #[test]
    fn notes_say_what_was_cleared_and_how_to_get_it_back() {
        let body = "a\n".repeat(240);
        assert_eq!(
            cleared_note("read", r#"{"path": "src/a.py"}"#, &body),
            "(Result cleared to save context: read src/a.py, 240 lines. Run it again if you need it.)"
        );
        assert!(
            cleared_note("read", r#"{"path": "a", "offset": 200}"#, "x")
                .contains("read a from line 200, 1 line.")
        );
        let long = format!("cargo test {}\nsecond line", "x".repeat(100));
        let n = cleared_note(
            "bash",
            &serde_json::json!({"command": long}).to_string(),
            "o",
        );
        assert!(n.contains("bash `cargo test xxx"), "{n}");
        assert!(n.contains("...`, 1 line."), "{n}");
        assert!(
            cleared_note("grep", r#"{"pattern": "fn", "intent": "look"}"#, "x")
                .contains(r#"grep {"pattern":"fn"}, 1 line"#)
        );
    }

    fn turn(id: &str, size: usize) -> Vec<ChatMessage> {
        vec![
            ChatMessage::Assistant {
                text: String::new(),
                tool_calls: vec![RawToolCall {
                    id: id.into(),
                    name: "read".into(),
                    arguments: r#"{"path": "f"}"#.into(),
                }],
            },
            ChatMessage::Tool {
                call_id: id.into(),
                content: "x".repeat(size),
            },
        ]
    }

    #[test]
    fn clears_oldest_first_and_keeps_the_last_three() {
        let mut m = vec![ChatMessage::User("task".into())];
        let mut src = vec![None];
        for (i, size) in [3000, 100, 3000, 3000, 3000, 3000, 3000].iter().enumerate() {
            // The same call id every turn, as some servers send.
            m.extend(turn("call_0", *size));
            src.extend([None, Some(format!("e{i}"))]);
        }
        // Enough for one: the oldest large one.
        let (ids, freed) = plan_clear(&m, &src, 500, 100).unwrap();
        assert_eq!(ids, ["e0"]);
        assert!(freed > 900, "{freed}");
        // Everything old: the short one and the last three stay.
        let (ids, _) = plan_clear(&m, &src, 100_000, 100).unwrap();
        assert_eq!(ids, ["e0", "e2", "e3"]);
        // Too little to be worth breaking the cache for.
        assert_eq!(plan_clear(&m, &src, 100_000, 10_000), None);
    }
}
