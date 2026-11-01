//! Keeping the context in the window (psi-plan.md §7.4). Past the trigger,
//! old tool results are first replaced by a one-line note that says what
//! they were and how to get them back (no model call). When that can't
//! free enough, the entries before a verbatim tail are summarized, and the
//! summary joins a chain of earlier ones (D-40); the two oldest are merged
//! when the chain grows too long.

use std::collections::HashMap;

use psi_ai::ChatMessage;
use psi_session::{Block, Entry, EntryBody, Role, text_of};
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

/// The characters an entry adds to a request.
fn sent_chars(e: &Entry) -> usize {
    match &e.body {
        EntryBody::Message { content, .. }
        | EntryBody::ToolResult { content, .. }
        | EntryBody::CustomMessage { content, .. } => content
            .iter()
            .map(|b| match b {
                Block::Text { text } => text.len(),
                Block::Thinking { .. } => 0,
                Block::ToolCall {
                    name,
                    arguments,
                    raw,
                    ..
                } => {
                    name.len()
                        + raw
                            .as_ref()
                            .map_or_else(|| arguments.to_string().len(), String::len)
                }
            })
            .sum(),
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

/// The summaries that stand in for the entries before the cut, oldest
/// first, and where the part sent as is starts.
#[derive(Debug, Default)]
pub(crate) struct Chain<'a> {
    /// (compaction entry id, summary)
    pub summaries: Vec<(&'a str, &'a str)>,
    /// Some entries were dropped without a summary (server overflow).
    pub dropped: bool,
    /// Index in the branch of the first entry sent as is.
    pub keep_from: usize,
}

pub(crate) fn summary_chain<'a>(branch: &[&'a Entry]) -> Chain<'a> {
    let mut chain = Chain::default();
    let mut keep_id = None;
    for e in branch {
        let EntryBody::Compaction {
            summary,
            first_kept_id,
            replaces,
            ..
        } = &e.body
        else {
            continue;
        };
        keep_id = Some(first_kept_id.as_str());
        if summary.is_empty() {
            chain.dropped = true;
            continue;
        }
        // A merge takes the place of the oldest summary it replaces.
        let replaced = |id: &str| replaces.iter().any(|r| r == id);
        let at = chain.summaries.iter().position(|(id, _)| replaced(id));
        chain.summaries.retain(|(id, _)| !replaced(id));
        let block = (e.id.as_str(), summary.as_str());
        match at {
            Some(i) => chain.summaries.insert(i, block),
            None => chain.summaries.push(block),
        }
    }
    chain.keep_from = keep_id
        .and_then(|id| branch.iter().position(|e| e.id == id))
        .unwrap_or(0);
    if chain.keep_from == 0 {
        chain.summaries.clear();
        chain.dropped = false;
    }
    chain
}

/// What the harness itself knows about the work before the cut (§7.4):
/// the files read and changed (from the read, write and edit calls), the
/// todo list and the steps of a plan being carried out. Sent after the
/// summaries instead of trusting them to keep it. Taken as of the cut, so
/// it changes only when a compaction does and the prompt cache holds.
pub(crate) fn harness_state(before: &[&Entry]) -> Option<String> {
    let mut read: Vec<&str> = Vec::new();
    let mut changed: Vec<&str> = Vec::new();
    for e in before.iter().rev() {
        for (_, name, args) in e.tool_calls() {
            let list = match name {
                "read" => &mut read,
                "write" | "edit" => &mut changed,
                _ => continue,
            };
            if let Some(p) = args["path"].as_str()
                && !list.contains(&p)
                && list.len() < MAX_STATE_FILES
            {
                list.push(p);
            }
        }
    }
    let mut parts = Vec::new();
    for (label, mut list) in [("Files read", read), ("Files changed", changed)] {
        if !list.is_empty() {
            list.reverse();
            parts.push(format!("{label}: {}", list.join(", ")));
        }
    }
    let custom = |kind: &str| {
        before.iter().rev().find_map(|e| match &e.body {
            EntryBody::Custom { custom_type, data } if custom_type == kind => Some(data),
            _ => None,
        })
    };
    if let Some(d) = custom("todo") {
        let todos = psi_tools::TodoList::from_json(d);
        if !todos.items.is_empty() {
            parts.push(format!("Todo list:\n{}", todos.render()));
        }
    }
    if let Some(d) = custom("plan") {
        let plan = crate::plan::PlanState::from_json(d);
        if plan.executing && !plan.steps.is_empty() {
            let steps: Vec<String> = plan
                .steps
                .iter()
                .enumerate()
                .map(|(i, s)| format!("[{}] {}. {}", if s.done { "x" } else { " " }, i + 1, s.text))
                .collect();
            parts.push(format!("Plan being carried out:\n{}", steps.join("\n")));
        }
    }
    (!parts.is_empty()).then(|| format!("{STATE_HEADER}\n{}", parts.join("\n")))
}

/// Heads the harness's own record in the context after a compaction.
pub const STATE_HEADER: &str = "[Recorded by the harness for the earlier conversation]";
/// The most files of each kind the harness state lists (the latest ones).
const MAX_STATE_FILES: usize = 30;

/// Tokens kept as they are after a summary: the larger of 20% of the
/// window and 20K (pi's keepRecentTokens), but at most half of what is
/// left under the trigger after `overhead` (system prompt, tools,
/// summaries), so a compaction leaves room to work before the next one.
pub(crate) fn tail_tokens(n_ctx: u64, trigger: u64, overhead: u64) -> u64 {
    (n_ctx / 5)
        .max(20_000)
        .min(trigger.saturating_sub(overhead) / 2)
}

/// A user message or a model step: where a kept part may start without
/// splitting a call from its result.
fn turn_start(e: &Entry) -> bool {
    e.is_model_step()
        || matches!(
            e.body,
            EntryBody::Message {
                role: Role::User,
                ..
            }
        )
}

/// Where the new kept part starts (a branch index after `from`, the
/// current one): the earliest turn start whose entries to the end fit in
/// `tail` tokens, or the last turn start when even that one doesn't fit.
/// `None` when that leaves no model step to summarize.
pub(crate) fn plan_cut(branch: &[&Entry], from: usize, tail: u64) -> Option<usize> {
    let mut chars = 0usize;
    let mut cut = None;
    for i in (from + 1..branch.len()).rev() {
        chars += sent_chars(branch[i]);
        if turn_start(branch[i]) {
            let fits = chars as u64 / CHARS_PER_TOKEN <= tail;
            if fits || cut.is_none() {
                cut = Some(i);
            }
            if !fits {
                break;
            }
        }
    }
    let cut = cut?;
    branch[from..cut]
        .iter()
        .any(|e| e.is_model_step())
        .then_some(cut)
}

/// How long a summary may be, in words: about half of a sixteenth of the
/// window (512 to 4096 tokens), at about 1.3 tokens a word.
pub(crate) fn summary_words(n_ctx: u64) -> u64 {
    (n_ctx / 16).clamp(512, 4096) * 3 / 4 / 2
}

/// The length and headings every summary is asked for.
fn summary_spec(words: u64) -> String {
    format!(
        "under {words} words, with these headings:\n\n\
         ## Goal\n\
         ## Constraints and facts\n\
         (every name, number, path and requirement the user gave, exactly as given)\n\
         ## Decisions\n\
         ## Current state\n\
         (what is done, what works, what fails)\n\
         ## Open problems\n\
         ## Next step"
    )
}

/// The instruction appended to the conversation to get a summary (D-38).
pub(crate) fn summary_instruction(after_summary: bool, words: u64) -> String {
    let what = if after_summary {
        "everything after the last summary"
    } else {
        "the conversation so far"
    };
    format!(
        "Stop the task for a moment: older messages are about to be removed from this conversation, and you will continue from your summary of them. Do not call tools now. Summarize {what} for yourself, {}",
        summary_spec(words)
    )
}

/// The request text of a summary built from text (the fallback).
pub(crate) fn plain_summary_prompt(conversation: &str, words: u64) -> String {
    format!(
        "<conversation>\n{conversation}\n</conversation>\n\nSummarize this part of the agent's work for the agent, {}",
        summary_spec(words)
    )
}

/// The system prompt of a summary request built from text (the fallback).
pub(crate) const SUMMARIZER_SYSTEM: &str = "You write summaries of a coding agent's work, so the agent can continue from them. Write only the summary.";

/// The entries as text for a summary request built from text (pi's way):
/// each tool result cut to about 2,000 characters, and the whole cut in
/// the middle to `max_chars`.
pub(crate) fn flatten(entries: &[&Entry], max_chars: usize) -> String {
    let mut out = String::new();
    for e in entries {
        let part = match &e.body {
            EntryBody::Message {
                role: Role::User,
                content,
                ..
            } => format!("User: {}", text_of(content)),
            EntryBody::Message {
                role: Role::Assistant,
                content,
                ..
            } if !content.is_empty() => {
                let mut t = format!("Assistant: {}", text_of(content));
                for (_, name, args) in e.tool_calls() {
                    t.push_str(&format!("\n[called {name} {args}]"));
                }
                t
            }
            EntryBody::ToolResult {
                tool_name, content, ..
            } => format!(
                "Result of {tool_name}: {}",
                cut_chars(&text_of(content), 2000)
            ),
            EntryBody::CustomMessage { content, .. } => format!("Note: {}", text_of(content)),
            _ => continue,
        };
        out.push_str(&part);
        out.push_str("\n\n");
    }
    cut_chars(out.trim_end(), max_chars)
}

/// Keeps both ends of `s`, about `max` characters in all.
fn cut_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_owned();
    }
    let head: String = s.chars().take(max / 2).collect();
    let tail: String = s.chars().skip(n - max / 2).collect();
    format!("{head}\n[... {} characters cut ...]\n{tail}", n - max)
}

/// The request text that merges the two oldest summaries into one.
pub(crate) fn merge_prompt(older: &str, newer: &str, words: u64) -> String {
    format!(
        "Merge these two summaries of consecutive parts of a coding agent's work into one, under {words} words, with the same headings. Keep every name, number, path and requirement exactly; where they disagree, the second is newer.\n\n<summary>\n{older}\n</summary>\n\n<summary>\n{newer}\n</summary>"
    )
}

/// A summary is usable when the model answered in text, did not call a
/// tool, and was not cut off. The reason when it isn't.
pub(crate) fn check_summary(r: &psi_ai::Response) -> Result<String, String> {
    if !r.tool_calls.is_empty() {
        return Err("the model called a tool".into());
    }
    if r.finish == psi_ai::FinishReason::Length {
        return Err("the summary was cut off by the output limit".into());
    }
    let t = r.text.trim();
    if t.is_empty() {
        return Err("the summary was empty".into());
    }
    Ok(t.to_owned())
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

    fn entries(bodies: Vec<EntryBody>) -> Vec<Entry> {
        let mut ids = psi_session::IdGen::new();
        bodies.into_iter().map(|b| ids.push(0, b)).collect()
    }

    fn msg(role: Role, text: &str) -> EntryBody {
        EntryBody::Message {
            role,
            content: vec![Block::Text { text: text.into() }],
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        }
    }

    fn compaction(summary: &str, keep: &str, replaces: &[&str]) -> EntryBody {
        EntryBody::Compaction {
            summary: summary.into(),
            first_kept_id: keep.into(),
            tokens_before: None,
            replaces: replaces.iter().map(|r| (*r).to_owned()).collect(),
        }
    }

    #[test]
    fn the_chain_keeps_order_and_a_merge_takes_the_oldest_place() {
        // ids 00000001.. in order
        let es = entries(vec![
            msg(Role::User, "task"),                                  // 1
            msg(Role::Assistant, "a"),                                // 2
            compaction("S1", "00000002", &[]),                        // 3
            msg(Role::Assistant, "b"),                                // 4
            compaction("S2", "00000004", &[]),                        // 5
            msg(Role::Assistant, "c"),                                // 6
            compaction("S3", "00000006", &[]),                        // 7
            compaction("M12", "00000006", &["00000003", "00000005"]), // 8
        ]);
        let refs: Vec<&Entry> = es.iter().collect();
        let c = summary_chain(&refs);
        let texts: Vec<&str> = c.summaries.iter().map(|(_, s)| *s).collect();
        assert_eq!(texts, ["M12", "S3"]);
        assert_eq!(c.keep_from, 5);
        assert!(!c.dropped);
        // Before any compaction, nothing.
        let c = summary_chain(&refs[..2]);
        assert!(c.summaries.is_empty() && c.keep_from == 0);
    }

    #[test]
    fn the_cut_keeps_whole_turns_that_fit_the_tail() {
        let big = "x".repeat(3000); // 1000 tokens
        let mut bodies = vec![msg(Role::User, "task")];
        for _ in 0..6 {
            bodies.push(msg(Role::Assistant, &big));
        }
        let es = entries(bodies);
        let refs: Vec<&Entry> = es.iter().collect();
        // 2500 tokens of tail: the last two steps.
        assert_eq!(plan_cut(&refs, 0, 2500), Some(5));
        // Smaller than one step: the last step anyway.
        assert_eq!(plan_cut(&refs, 0, 10), Some(6));
        // Everything fits: nothing is left to summarize before the cut.
        assert_eq!(plan_cut(&refs, 0, 100_000), None);
        // Only after the current cut, and only with a step to summarize.
        assert_eq!(plan_cut(&refs, 5, 2500), Some(6));
        assert_eq!(plan_cut(&refs, 6, 2500), None);
    }

    #[test]
    fn the_tail_is_pis_size_unless_the_window_is_small() {
        // 64K: 20K, as in pi.
        assert_eq!(tail_tokens(65_536, 45_875, 3000), 20_000);
        // 16K: half of what is left under the trigger.
        assert_eq!(tail_tokens(16_384, 9831, 1831), 4000);
    }

    #[test]
    fn flattened_results_are_cut() {
        let es = entries(vec![
            msg(Role::User, "task"),
            EntryBody::ToolResult {
                tool_call_id: "c".into(),
                tool_name: "bash".into(),
                content: vec![Block::Text {
                    text: "y".repeat(5000),
                }],
                is_error: false,
            },
        ]);
        let refs: Vec<&Entry> = es.iter().collect();
        let t = flatten(&refs, 100_000);
        assert!(t.starts_with("User: task\n\nResult of bash: yyy"));
        assert!(t.contains("[... 3000 characters cut ...]"));
        assert!(flatten(&refs, 500).chars().count() < 600);
    }

    fn step_with(name: &str, path: &str) -> EntryBody {
        EntryBody::Message {
            role: Role::Assistant,
            content: vec![Block::ToolCall {
                id: "c".into(),
                name: name.into(),
                arguments: serde_json::json!({"path": path}),
                raw: None,
            }],
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        }
    }

    #[test]
    fn harness_state_lists_files_todos_and_the_plan() {
        let es = entries(vec![
            msg(Role::User, "task"),
            step_with("read", "a.py"),
            step_with("edit", "a.py"),
            step_with("read", "b.py"),
            step_with("read", "a.py"),
            step_with("write", "c.py"),
            EntryBody::Custom {
                custom_type: "todo".into(),
                data: psi_tools::TodoList::default().to_json(),
            },
            EntryBody::Custom {
                custom_type: "todo".into(),
                data: serde_json::json!({"todos": [{"id": 1, "text": "port", "done": true},
                                                    {"id": 2, "text": "test", "done": false}],
                                          "next_id": 3}),
            },
            EntryBody::Custom {
                custom_type: "plan".into(),
                data: serde_json::json!({"enabled": false, "executing": true,
                    "steps": [{"text": "one", "done": true}, {"text": "two", "done": false}]}),
            },
        ]);
        let refs: Vec<&Entry> = es.iter().collect();
        assert_eq!(
            harness_state(&refs).unwrap(),
            "[Recorded by the harness for the earlier conversation]\n\
             Files read: b.py, a.py\n\
             Files changed: a.py, c.py\n\
             Todo list:\n[x] #1: port\n[ ] #2: test\n\
             Plan being carried out:\n[x] 1. one\n[ ] 2. two"
        );
        assert_eq!(harness_state(&refs[..1]), None);
    }
}
