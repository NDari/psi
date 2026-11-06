//! The session branch as chat messages, and dropping old turns when the
//! server says the request is too large (psi-plan.md §4.2).

use std::collections::{HashMap, HashSet};

use psi_ai::{ChatMessage, RawToolCall};
use psi_session::{Block, Entry, EntryBody, Role, text_of};

use crate::compact::{Chain, cleared_note, harness_state, summary_chain};

/// Sent in place of turns dropped to fit the context window.
pub const DROPPED_NOTE: &str =
    "(Earlier turns of this conversation were removed to fit the context window.)";

/// Builds the request messages: the system prompt, then the branch. After
/// a compaction, the entries before its cut are sent as the chain of
/// summaries, then the last user message before the cut when the cut falls
/// inside its turn (§7.4). Cleared tool results are sent as a note.
/// Consecutive user messages are merged, and a tool call without a result
/// gets one, so every chat template accepts the conversation.
pub fn build_messages(system: &str, branch: &[&Entry]) -> Vec<ChatMessage> {
    build_with_sources(system, branch).0
}

/// `build_messages`, plus for each message the id of the tool result entry
/// it was built from, if it was.
pub(crate) fn build_with_sources(
    system: &str,
    branch: &[&Entry],
) -> (Vec<ChatMessage>, Vec<Option<String>>) {
    let mut out = vec![ChatMessage::System(system.to_owned())];
    let mut sources: Vec<Option<String>> = Vec::new();
    let chain = summary_chain(branch);
    if chain.keep_from > 0 {
        let opening = opening_text(
            &chain,
            &branch[..chain.keep_from],
            branch.get(chain.keep_from),
        );
        push_user(&mut out, &opening);
    }
    let cleared: HashSet<&str> = branch
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::ClearedResults { entry_ids, .. } => Some(entry_ids),
            _ => None,
        })
        .flatten()
        .map(String::as_str)
        .collect();
    let mut calls: HashMap<String, (String, String)> = HashMap::new();
    let mut pending: Vec<String> = Vec::new();
    let mut answered: HashSet<&str> = HashSet::new();
    for e in &branch[chain.keep_from..] {
        match &e.body {
            EntryBody::Message {
                role: Role::User,
                content,
                ..
            } => {
                close_calls(&mut out, &mut pending, &answered);
                push_user(&mut out, &text_of(content));
            }
            EntryBody::CustomMessage { content, .. } => {
                close_calls(&mut out, &mut pending, &answered);
                push_user(&mut out, &text_of(content));
            }
            EntryBody::Message {
                role: Role::Assistant,
                content,
                ..
            } => {
                if content.is_empty() {
                    continue; // a failed or aborted request
                }
                close_calls(&mut out, &mut pending, &answered);
                let mut text = String::new();
                let mut tool_calls = Vec::new();
                for b in content {
                    match b {
                        Block::Text { text: t } => text.push_str(t),
                        Block::ToolCall {
                            id,
                            name,
                            arguments,
                            raw,
                        } => {
                            // An id can come again (servers without ids get
                            // call_0, call_1... each response): this call
                            // isn't answered until its own result comes.
                            answered.remove(id.as_str());
                            pending.push(id.clone());
                            let arguments = history_arguments(raw.as_deref(), arguments);
                            calls.insert(id.clone(), (name.clone(), arguments.clone()));
                            tool_calls.push(RawToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments,
                            });
                        }
                        // Thinking is never sent back (strip_from_history).
                        Block::Thinking { .. } => {}
                    }
                }
                out.push(ChatMessage::Assistant { text, tool_calls });
            }
            EntryBody::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                answered.insert(tool_call_id);
                let mut content = text_of(content);
                if cleared.contains(e.id.as_str()) {
                    let (name, args) = calls
                        .get(tool_call_id)
                        .map_or(("tool", "{}"), |(n, a)| (n.as_str(), a.as_str()));
                    content = cleared_note(name, args, &content);
                }
                sources.resize(out.len(), None);
                sources.push(Some(e.id.clone()));
                out.push(ChatMessage::Tool {
                    call_id: tool_call_id.clone(),
                    content,
                });
            }
            _ => {}
        }
    }
    close_calls(&mut out, &mut pending, &answered);
    sources.resize(out.len(), None);
    (out, sources)
}

/// The arguments to send back for a past call: as the model wrote them, so
/// the prompt cache holds, unless they are not valid JSON (a call cut off by
/// the output limit). llama-server rejects a request whose history has such
/// arguments, so those go back as `{}`; the call's result says what happened.
fn history_arguments(raw: Option<&str>, parsed: &serde_json::Value) -> String {
    match raw {
        Some(r) if serde_json::from_str::<serde_json::Value>(r).is_ok() => r.to_owned(),
        Some(_) => "{}".into(),
        None if parsed.is_object() => parsed.to_string(),
        None => "{}".into(),
    }
}

/// What stands in for the entries before the cut: the summaries, oldest
/// first, a note if turns were dropped without one, the harness's own
/// record (files, todo, plan), and the last user message before the cut,
/// unless the kept part starts with a user message.
fn opening_text(chain: &Chain, before: &[&Entry], first_kept: Option<&&Entry>) -> String {
    let mut parts: Vec<String> = chain
        .summaries
        .iter()
        .map(|(_, s)| format!("{SUMMARY_HEADER}\n\n{s}"))
        .collect();
    if chain.dropped {
        parts.push(DROPPED_NOTE.into());
    }
    if let Some(state) = harness_state(before) {
        parts.push(state);
    }
    let starts_with_user = first_kept.is_some_and(|e| {
        matches!(
            e.body,
            EntryBody::Message {
                role: Role::User,
                ..
            }
        )
    });
    let last_user = before.iter().rev().find_map(|e| match &e.body {
        EntryBody::Message {
            role: Role::User,
            content,
            ..
        } => Some(text_of(content)),
        _ => None,
    });
    if let (false, Some(t)) = (starts_with_user, last_user) {
        parts.push(format!("{RESTATED_HEADER}\n\n{t}"));
    }
    parts.join("\n\n")
}

/// Heads each summary in the context.
pub const SUMMARY_HEADER: &str = "[Summary of earlier work in this conversation]";
/// Heads the user message repeated after the summaries.
pub const RESTATED_HEADER: &str = "[The last user message, repeated]";

fn push_user(out: &mut Vec<ChatMessage>, text: &str) {
    if let Some(ChatMessage::User(prev)) = out.last_mut() {
        prev.push_str("\n\n");
        prev.push_str(text);
    } else {
        out.push(ChatMessage::User(text.to_owned()));
    }
}

/// Gives calls left without a result (an abort) a result.
fn close_calls(out: &mut Vec<ChatMessage>, pending: &mut Vec<String>, answered: &HashSet<&str>) {
    for id in pending.drain(..) {
        if !answered.contains(id.as_str()) {
            out.push(ChatMessage::Tool {
                call_id: id,
                content: "(no result: the run was stopped before this call ran)".into(),
            });
        }
    }
}

/// Picks the first turn to keep so the request fits: drops the oldest turns
/// (each starts at an assistant message) of the part sent as is, after the
/// task when nothing was cut yet (after a cut, the opening repeats the last
/// user message). `None` when even the last turn alone is too large.
pub(crate) fn plan_drop(
    branch: &[&Entry],
    n_prompt: u64,
    n_ctx: u64,
    max_output: u64,
) -> Option<String> {
    let chain = summary_chain(branch);
    let sent = &branch[chain.keep_from..];
    let target = n_ctx.saturating_sub(max_output).max(n_ctx / 2);
    let summary_chars: usize = chain.summaries.iter().map(|(_, s)| s.len()).sum();
    let total_chars = (sent.iter().map(|e| size(e)).sum::<usize>() + summary_chars).max(1);
    let chars_per_token = total_chars as f64 / n_prompt.max(1) as f64;
    let cut_tokens = n_prompt.saturating_sub(target) as f64;
    let need = (cut_tokens * chars_per_token * 1.1) as usize;
    let skip = if chain.keep_from == 0 {
        let first_user = sent.iter().position(|e| {
            matches!(
                e.body,
                EntryBody::Message {
                    role: Role::User,
                    ..
                }
            )
        })?;
        first_user + 1
    } else {
        0
    };
    let starts: Vec<usize> = sent
        .iter()
        .enumerate()
        .skip(skip)
        .filter(|(_, e)| e.is_model_step())
        .map(|(i, _)| i)
        .collect();
    let mut cut = 0;
    // Keep at least the last turn.
    for w in starts.windows(2) {
        cut += sent[w[0]..w[1]].iter().map(|e| size(e)).sum::<usize>();
        if cut >= need {
            return Some(sent[w[1]].id.clone());
        }
    }
    None
}

fn size(e: &Entry) -> usize {
    match &e.body {
        EntryBody::Message { content, .. }
        | EntryBody::ToolResult { content, .. }
        | EntryBody::CustomMessage { content, .. } => content
            .iter()
            .map(|b| match b {
                Block::Text { text } => text.len(),
                Block::Thinking { .. } => 0,
                Block::ToolCall { arguments, raw, .. } => raw
                    .as_ref()
                    .map_or_else(|| arguments.to_string().len(), String::len),
            })
            .sum(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::STATE_HEADER;
    use psi_session::IdGen;
    use serde_json::json;

    fn user(t: &str) -> EntryBody {
        EntryBody::Message {
            role: Role::User,
            content: vec![Block::Text { text: t.into() }],
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        }
    }
    fn asst(text: &str, call: Option<&str>) -> EntryBody {
        let mut content = vec![Block::Text { text: text.into() }];
        if let Some(id) = call {
            content.push(Block::ToolCall {
                id: id.into(),
                name: "read".into(),
                arguments: json!({"path": "a"}),
                raw: Some("{\"path\": \"a\"}".into()),
            });
        }
        EntryBody::Message {
            role: Role::Assistant,
            content,
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        }
    }
    fn result(id: &str, t: &str) -> EntryBody {
        EntryBody::ToolResult {
            tool_call_id: id.into(),
            tool_name: "read".into(),
            content: vec![Block::Text { text: t.into() }],
            is_error: false,
        }
    }

    fn entries(bodies: Vec<EntryBody>) -> Vec<Entry> {
        let mut ids = IdGen::new();
        bodies.into_iter().map(|b| ids.push(0, b)).collect()
    }

    #[test]
    fn a_reused_call_id_without_a_result_still_gets_one() {
        // call_0 answered in the first turn, then call_0 again cut off by
        // an abort: the second needs its own placeholder result.
        let es = entries(vec![
            user("task"),
            asst("", Some("call_0")),
            result("call_0", "first"),
            asst("", Some("call_0")),
            user("again"),
        ]);
        let refs: Vec<&Entry> = es.iter().collect();
        let m = build_messages("sys", &refs);
        let tools: Vec<&ChatMessage> = m
            .iter()
            .filter(|x| matches!(x, ChatMessage::Tool { .. }))
            .collect();
        assert_eq!(tools.len(), 2, "{m:#?}");
        assert!(
            matches!(&m[5], ChatMessage::Tool { content, .. } if content.starts_with("(no result")),
            "{m:#?}"
        );
    }

    #[test]
    fn branch_to_messages_with_raw_arguments() {
        let es = entries(vec![
            user("task"),
            asst("", Some("c1")),
            result("c1", "data"),
            asst("done", None),
        ]);
        let refs: Vec<&Entry> = es.iter().collect();
        let m = build_messages("sys", &refs);
        assert_eq!(
            m,
            vec![
                ChatMessage::System("sys".into()),
                ChatMessage::User("task".into()),
                ChatMessage::Assistant {
                    text: String::new(),
                    tool_calls: vec![RawToolCall {
                        id: "c1".into(),
                        name: "read".into(),
                        arguments: "{\"path\": \"a\"}".into()
                    }]
                },
                ChatMessage::Tool {
                    call_id: "c1".into(),
                    content: "data".into()
                },
                ChatMessage::Assistant {
                    text: "done".into(),
                    tool_calls: vec![]
                },
            ]
        );
    }

    #[test]
    fn unanswered_calls_get_a_result_and_users_merge() {
        let es = entries(vec![user("task"), asst("", Some("c1")), user("steer")]);
        let refs: Vec<&Entry> = es.iter().collect();
        let m = build_messages("s", &refs);
        assert!(matches!(&m[3], ChatMessage::Tool { call_id, .. } if call_id == "c1"));
        assert_eq!(m[4], ChatMessage::User("steer".into()));
        let es = entries(vec![user("a"), user("b")]);
        let refs: Vec<&Entry> = es.iter().collect();
        assert_eq!(
            build_messages("s", &refs)[1],
            ChatMessage::User("a\n\nb".into())
        );
    }

    #[test]
    fn dropping_keeps_the_task_and_recent_turns() {
        let big = "x".repeat(1000);
        let mut bodies = vec![user("task")];
        for i in 0..5 {
            let id = format!("c{i}");
            bodies.push(asst("", Some(&id)));
            bodies.push(result(&id, &big));
        }
        let mut es = entries(bodies);
        let refs: Vec<&Entry> = es.iter().collect();
        // 5000+ chars as 2000 tokens in a 1500-token window with 500 for output.
        let keep = plan_drop(&refs, 2000, 1500, 500).unwrap();
        let mut ids = IdGen::resume(&es);
        es.push(ids.push(
            0,
            EntryBody::Compaction {
                summary: String::new(),
                first_kept_id: keep.clone(),
                tokens_before: Some(2000),
                replaces: vec![],
            },
        ));
        let refs: Vec<&Entry> = es.iter().collect();
        let m = build_messages("s", &refs);
        assert_eq!(
            m[1],
            ChatMessage::User(format!(
                "{DROPPED_NOTE}\n\n{STATE_HEADER}\nFiles read: a\n\n{RESTATED_HEADER}\n\ntask"
            ))
        );
        let kept_turns = m
            .iter()
            .filter(|x| matches!(x, ChatMessage::Assistant { .. }))
            .count();
        assert!((1..5).contains(&kept_turns), "{kept_turns}");
        // Too large even with one turn left.
        let refs: Vec<&Entry> = es[..3].iter().collect();
        assert_eq!(plan_drop(&refs, 2000, 1500, 500), None);
    }

    #[test]
    fn dropping_after_a_summary_drops_only_what_is_sent() {
        let big = "x".repeat(1000);
        let mut bodies = vec![user("task")];
        for i in 0..8 {
            let id = format!("c{i}");
            bodies.push(asst("", Some(&id)));
            bodies.push(result(&id, &big));
        }
        let mut es = entries(bodies);
        // A summary stands in for the first four turns.
        let summary_keep = es[9].id.clone();
        let mut ids = IdGen::resume(&es);
        es.push(ids.push(
            0,
            EntryBody::Compaction {
                summary: "did four reads".into(),
                first_kept_id: summary_keep.clone(),
                tokens_before: None,
                replaces: vec![],
            },
        ));
        let refs: Vec<&Entry> = es.iter().collect();
        let keep = plan_drop(&refs, 2000, 1500, 500).unwrap();
        let at = |id: &str| es.iter().position(|e| e.id == id).unwrap();
        assert!(at(&keep) > at(&summary_keep), "the cut moved back");
        es.push(ids.push(
            0,
            EntryBody::Compaction {
                summary: String::new(),
                first_kept_id: keep,
                tokens_before: Some(2000),
                replaces: vec![],
            },
        ));
        let refs: Vec<&Entry> = es.iter().collect();
        let m = build_messages("s", &refs);
        let ChatMessage::User(opening) = &m[1] else {
            panic!("{m:?}")
        };
        assert!(opening.contains("did four reads") && opening.contains(DROPPED_NOTE));
        let kept_turns = m
            .iter()
            .filter(|x| matches!(x, ChatMessage::Assistant { .. }))
            .count();
        assert!((1..4).contains(&kept_turns), "{kept_turns}");
    }
}
