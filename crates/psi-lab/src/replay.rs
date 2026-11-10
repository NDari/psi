//! `psi replay` (psi-plan.md §12.4; design-going-forward.md, goal 4): a
//! recorded request sent again, as it was or with an edit, and replay
//! scripts from recorded runs.

use std::collections::BTreeMap;
use std::path::Path;

use psi_ai::replay::{Reply, ScriptedReply};
use psi_ai::wire::{WireKind, read_wire};
use psi_ai::{Client, Response};
use psi_session::{Block, Entry, EntryBody, Role, StopReason};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::error::other;

/// The model's side of a recorded run, as replies for the replay server:
/// each assistant message with content becomes one scripted answer.
pub fn script_from_entries(entries: &[Entry]) -> Vec<Reply> {
    entries
        .iter()
        .filter(|e| e.is_model_step())
        .filter_map(|e| match &e.body {
            EntryBody::Message {
                role: Role::Assistant,
                content,
                stop_reason,
                usage,
                ..
            } => {
                let mut r = ScriptedReply::default();
                for b in content {
                    match b {
                        Block::Text { text } => r.text.push_str(text),
                        Block::ToolCall {
                            name,
                            arguments,
                            raw,
                            ..
                        } => r.tool_calls.push((
                            name.clone(),
                            raw.clone().unwrap_or_else(|| arguments.to_string()),
                        )),
                        Block::Thinking { .. } => {}
                    }
                }
                if *stop_reason == Some(StopReason::Length) {
                    r.finish = Some("length".into());
                }
                if let Some(u) = usage {
                    r.prompt_tokens = u.prompt_total();
                    r.completion_tokens = u.output;
                }
                Some(Reply::Chat(r))
            }
            _ => None,
        })
        .collect()
}

/// The exact body of the `step`-th chat request (from 1) in a wire log.
pub fn request_at(wire: &Path, step: usize) -> Result<Vec<u8>> {
    let entries = read_wire(wire).map_err(|e| other(format!("{}: {e}", wire.display())))?;
    let reqs: Vec<_> = entries
        .into_iter()
        .filter(|e| e.kind == WireKind::Request && e.url.ends_with("/chat/completions"))
        .collect();
    if step == 0 || step > reqs.len() {
        return Err(other(format!(
            "step {step} is out of range: the wire log has {} requests",
            reqs.len()
        )));
    }
    Ok(reqs[step - 1].body.clone().into_bytes())
}

/// A change to a recorded request before it is sent again: the way to try
/// a wording, a note or a cut against what the model really saw.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// Add a user message at the end.
    Append(String),
    /// Replace text wherever it is in the messages and the tool
    /// descriptions (the system prompt, a tool result, a tool's text).
    Replace { old: String, new: String },
    /// Remove the last messages.
    DropLast(usize),
    /// Set a field of the request itself, such as `temperature`.
    Set { key: String, value: Value },
}

fn replace_in(v: &mut Value, old: &str, new: &str) -> usize {
    match v {
        Value::String(s) => {
            let n = s.matches(old).count();
            if n > 0 {
                *s = s.replace(old, new);
            }
            n
        }
        Value::Array(a) => a.iter_mut().map(|x| replace_in(x, old, new)).sum(),
        Value::Object(o) => o.values_mut().map(|x| replace_in(x, old, new)).sum(),
        _ => 0,
    }
}

/// The recorded request `body` with `edits` applied in order. An edit that
/// changes nothing is an error: the answer would be about the wrong request.
pub fn edit_request(body: &[u8], edits: &[Edit]) -> Result<Vec<u8>> {
    if edits.is_empty() {
        return Ok(body.to_vec());
    }
    let mut v: Value = serde_json::from_slice(body)
        .map_err(|e| other(format!("the recorded request is not JSON: {e}")))?;
    for edit in edits {
        match edit {
            Edit::Append(text) => {
                let messages = v["messages"]
                    .as_array_mut()
                    .ok_or_else(|| other("the recorded request has no messages"))?;
                messages.push(serde_json::json!({"role": "user", "content": text}));
            }
            Edit::Replace { old, new } => {
                let mut n = replace_in(&mut v["messages"], old, new);
                n += replace_in(&mut v["tools"], old, new);
                if n == 0 {
                    return Err(other(format!(
                        "--replace: {old:?} is not in the request's messages or tools"
                    )));
                }
            }
            Edit::DropLast(n) => {
                let messages = v["messages"]
                    .as_array_mut()
                    .ok_or_else(|| other("the recorded request has no messages"))?;
                // The system prompt and the first user message stay.
                if *n == 0 || messages.len() < n + 2 {
                    return Err(other(format!(
                        "--drop-last {n}: the request has {} messages, and the first two stay",
                        messages.len()
                    )));
                }
                messages.truncate(messages.len() - n);
            }
            Edit::Set { key, value } => v[key.as_str()] = value.clone(),
        }
    }
    Ok(serde_json::to_vec(&v).unwrap_or_default())
}

/// One line describing what a response does next.
pub fn action(r: &Response) -> String {
    if r.tool_calls.is_empty() {
        let t: String = r.text.trim().chars().take(80).collect();
        return format!("answer: {t}");
    }
    r.tool_calls
        .iter()
        .map(|c| {
            let args: String = c.arguments.chars().take(120).collect();
            format!("{} {args}", c.name)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Resends the request at `step` `samples` times. One sample sends the
/// exact bytes; several set a different `seed` in each. Returns each
/// sample's action, and the distribution of actions.
pub async fn replay(
    client: &Client,
    body: Vec<u8>,
    samples: u32,
) -> Result<(Vec<String>, BTreeMap<String, u32>)> {
    let mut actions = Vec::new();
    for i in 0..samples.max(1) {
        let bytes = if samples <= 1 {
            body.clone()
        } else {
            let mut v: Value = serde_json::from_slice(&body)
                .map_err(|e| other(format!("the recorded request is not JSON: {e}")))?;
            v["seed"] = Value::from(i);
            serde_json::to_vec(&v).unwrap_or_default()
        };
        let r = client
            .chat_bytes(bytes, &CancellationToken::new(), &mut |_| {})
            .await
            .map_err(|e| other(e.to_string()))?;
        actions.push(action(&r));
    }
    let mut dist = BTreeMap::new();
    for a in &actions {
        *dist.entry(a.clone()).or_insert(0) += 1;
    }
    Ok((actions, dist))
}

#[cfg(test)]
mod tests {
    use super::*;
    use psi_ai::replay::ReplayServer;

    const FIXTURE: &str = include_str!("../tests/fixtures/recorded-run.jsonl");

    #[test]
    fn a_recorded_run_becomes_a_script() {
        let entries = psi_session::read_entries(FIXTURE.as_bytes()).unwrap();
        let script = script_from_entries(&entries);
        assert_eq!(
            script.len(),
            2,
            "two model steps; the aborted request is left out"
        );
        let Reply::Chat(first) = &script[0] else {
            panic!()
        };
        assert_eq!(first.tool_calls.len(), 2);
        assert_eq!(first.tool_calls[0].0, "read");
        assert_eq!(first.prompt_tokens, 1692);
    }

    #[tokio::test]
    async fn replay_resends_exact_bytes_and_counts_actions() {
        let dir = tempfile::tempdir().unwrap();
        let server = ReplayServer::start(vec![
            Reply::Chat(ScriptedReply::text("first")),
            Reply::Chat(ScriptedReply::call(
                "read",
                serde_json::json!({"path": "a"}),
            )),
            Reply::Chat(ScriptedReply::call(
                "read",
                serde_json::json!({"path": "a"}),
            )),
        ])
        .unwrap();
        let wire = std::sync::Arc::new(psi_ai::WireLog::open(&dir.path().join("w.jsonl")).unwrap());
        let client = Client::new(&server.base_url()).with_wire(wire);
        let req = psi_ai::ChatRequest {
            model: "local".into(),
            messages: vec![psi_ai::ChatMessage::User("hi".into())],
            tools: vec![],
            max_tokens: 5,
            enable_thinking: None,
            sampling: Default::default(),
        };
        client
            .chat(&req, &CancellationToken::new(), &mut |_| {})
            .await
            .unwrap();

        let body = request_at(&dir.path().join("w.jsonl"), 1).unwrap();
        assert_eq!(body, req.to_bytes());
        assert!(request_at(&dir.path().join("w.jsonl"), 2).is_err());
        let plain = Client::new(&server.base_url());
        let (actions, dist) = replay(&plain, body, 2).await.unwrap();
        assert_eq!(actions, [r#"read {"path":"a"}"#, r#"read {"path":"a"}"#]);
        assert_eq!(dist.get(r#"read {"path":"a"}"#), Some(&2));
        let seeds: Vec<Value> = server
            .chat_requests()
            .iter()
            .skip(1)
            .map(|r| r["seed"].clone())
            .collect();
        assert_eq!(seeds, [Value::from(0), Value::from(1)]);
    }

    #[test]
    fn a_recorded_request_is_edited_before_it_is_sent_again() {
        let body = serde_json::json!({
            "model": "local",
            "messages": [
                {"role": "system", "content": "Be brief. Don't repeat calls."},
                {"role": "user", "content": "go"},
                {"role": "assistant", "content": "", "tool_calls": [
                    {"id": "c1", "function": {"name": "bash", "arguments": "{\"command\":\"ls\"}"}}]},
                {"role": "tool", "tool_call_id": "c1", "content": "a.txt\n[exit code 1]"},
            ],
            "tools": [{"type": "function", "function": {"name": "bash", "description": "Run a bash command"}}],
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        assert_eq!(
            edit_request(&bytes, &[]).unwrap(),
            bytes,
            "no edit: the exact bytes"
        );
        let edits = [
            Edit::Replace {
                old: "[exit code 1]".into(),
                new: "[exit code 1: nothing matched]".into(),
            },
            Edit::Replace {
                old: "Run a bash command".into(),
                new: "Run a command".into(),
            },
            Edit::Append("[Note from psi] Stop repeating.".into()),
            Edit::Set {
                key: "temperature".into(),
                value: serde_json::json!(1.0),
            },
        ];
        let v: Value = serde_json::from_slice(&edit_request(&bytes, &edits).unwrap()).unwrap();
        assert_eq!(
            v["messages"][3]["content"],
            "a.txt\n[exit code 1: nothing matched]"
        );
        assert_eq!(v["tools"][0]["function"]["description"], "Run a command");
        assert_eq!(
            v["messages"][4],
            serde_json::json!({"role": "user", "content": "[Note from psi] Stop repeating."})
        );
        assert_eq!(v["temperature"], 1.0);
        // The last turn cut out: the call and its result.
        let cut = edit_request(&bytes, &[Edit::DropLast(2)]).unwrap();
        let v: Value = serde_json::from_slice(&cut).unwrap();
        assert_eq!(v["messages"].as_array().unwrap().len(), 2);
        // An edit that changes nothing is an error, not a silent no-op.
        let missing = Edit::Replace {
            old: "not there".into(),
            new: "x".into(),
        };
        let err = edit_request(&bytes, &[missing]).unwrap_err().to_string();
        assert_eq!(
            err,
            "--replace: \"not there\" is not in the request's messages or tools"
        );
        assert!(edit_request(&bytes, &[Edit::DropLast(3)]).is_err());
    }
}
