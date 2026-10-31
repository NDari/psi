//! Session entry types (psi-plan.md §7.1).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One line of a session file. `parent_id` links entries into a tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub parent_id: Option<String>,
    /// Milliseconds since the Unix epoch.
    pub ts: u64,
    #[serde(flatten)]
    pub body: EntryBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EntryBody {
    /// A user, assistant or system message.
    Message {
        role: Role,
        content: Vec<Block>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_reason: Option<StopReason>,
        /// The provider's error text, when the request failed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// The result of one tool call.
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: Vec<Block>,
        #[serde(default)]
        is_error: bool,
    },
    /// Extension or harness data, not sent to the model.
    Custom {
        custom_type: String,
        #[serde(default)]
        data: Value,
    },
    /// An extension message that is sent to the model.
    CustomMessage {
        custom_type: String,
        content: Vec<Block>,
    },
    /// A summary that replaces the entries before `first_kept_id`.
    Compaction {
        summary: String,
        first_kept_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens_before: Option<u64>,
    },
    /// A user label on an entry; `None` clears it.
    Label {
        target_id: String,
        #[serde(default)]
        label: Option<String>,
    },
    /// A workspace snapshot in the shadow repository (§7.2).
    Checkpoint {
        commit: String,
        // TODO(phase-3): what triggered it and the tool call it follows.
    },
    /// Why a tool call ran, asked or was denied (§8), and any rewrite.
    PolicyDecision {
        tool_call_id: String,
        decision: String,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rewrite: Option<Value>,
    },
    /// A guard firing (§8.6).
    GuardEvent {
        guard: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
        /// The arguments exactly as the model sent them, when they differ
        /// from `arguments` (sent back as is, so the prompt cache holds).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw: Option<String>,
    },
}

/// Token counts for one model response.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
}

impl Usage {
    /// All prompt tokens, cached or not.
    pub fn prompt_total(&self) -> u64 {
        self.input + self.cache_read + self.cache_write
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The model finished its answer.
    Stop,
    /// The model called tools.
    ToolUse,
    /// The response hit the output limit.
    Length,
    Error,
    Aborted,
}

impl Entry {
    /// The tool calls in an assistant message, in order.
    pub fn tool_calls(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        let blocks: &[Block] = match &self.body {
            EntryBody::Message {
                role: Role::Assistant,
                content,
                ..
            } => content,
            _ => &[],
        };
        blocks.iter().filter_map(|b| match b {
            Block::ToolCall {
                id,
                name,
                arguments,
                ..
            } => Some((id.as_str(), name.as_str(), arguments)),
            _ => None,
        })
    }

    /// Whether this is an assistant message the model actually produced
    /// (a model step), as opposed to an empty failed or aborted request.
    pub fn is_model_step(&self) -> bool {
        matches!(&self.body, EntryBody::Message { role: Role::Assistant, content, .. } if !content.is_empty())
    }
}

/// Joins the text blocks of a content list.
pub fn text_of(content: &[Block]) -> String {
    let mut out = String::new();
    for b in content {
        if let Block::Text { text } = b {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
    }
    out
}

/// Hands out entry ids and chains each entry to the previous one.
#[derive(Debug, Default)]
pub struct IdGen {
    next: u64,
    last: Option<String>,
    /// Random ids, for live sessions whose branches must never collide.
    random: bool,
}

impl IdGen {
    pub fn new() -> Self {
        Self::default()
    }

    /// Continues after `entries` made by an `IdGen`, so new entries chain on.
    pub fn resume(entries: &[Entry]) -> Self {
        Self {
            next: entries.len() as u64,
            last: entries.last().map(|e| e.id.clone()),
            random: false,
        }
    }

    /// Random 8-hex-digit ids, continuing after `last`.
    pub fn random_after(last: Option<String>) -> Self {
        Self {
            next: 0,
            last,
            random: true,
        }
    }

    /// Wraps `body` in an entry whose parent is the previous entry.
    pub fn push(&mut self, ts: u64, body: EntryBody) -> Entry {
        self.next += 1;
        let id = if self.random {
            let u = uuid::Uuid::now_v7().simple().to_string();
            u[u.len() - 8..].to_owned()
        } else {
            format!("{:08x}", self.next)
        };
        let parent_id = self.last.replace(id.clone());
        Entry {
            id,
            parent_id,
            ts,
            body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entry_serializes_flat_with_type_tag() {
        let mut ids = IdGen::new();
        let e = ids.push(
            5,
            EntryBody::ToolResult {
                tool_call_id: "c1".into(),
                tool_name: "read".into(),
                content: vec![Block::Text { text: "hi".into() }],
                is_error: false,
            },
        );
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            json!({"id": "00000001", "parent_id": null, "ts": 5, "type": "tool_result",
                   "tool_call_id": "c1", "tool_name": "read",
                   "content": [{"type": "text", "text": "hi"}], "is_error": false})
        );
    }

    #[test]
    fn ids_chain_parents() {
        let mut ids = IdGen::new();
        let a = ids.push(0, EntryBody::Checkpoint { commit: "x".into() });
        let b = ids.push(0, EntryBody::Checkpoint { commit: "y".into() });
        assert_eq!(a.parent_id, None);
        assert_eq!(b.parent_id.as_deref(), Some(a.id.as_str()));
    }

    #[test]
    fn resume_continues_the_chain() {
        let mut ids = IdGen::new();
        let a = ids.push(0, EntryBody::Checkpoint { commit: "x".into() });
        let b = IdGen::resume(std::slice::from_ref(&a))
            .push(0, EntryBody::Checkpoint { commit: "y".into() });
        assert_eq!(b.id, "00000002");
        assert_eq!(b.parent_id.as_deref(), Some("00000001"));
    }

    #[test]
    fn every_entry_type_round_trips() {
        let bodies = vec![
            EntryBody::Message {
                role: Role::Assistant,
                content: vec![
                    Block::Thinking {
                        thinking: "t".into(),
                    },
                    Block::Text { text: "a".into() },
                    Block::ToolCall {
                        id: "c1".into(),
                        name: "bash".into(),
                        arguments: json!({"command": "ls"}),
                        raw: Some("{\"command\":\"ls\"}".into()),
                    },
                ],
                usage: Some(Usage {
                    input: 1,
                    output: 2,
                    cache_read: 3,
                    cache_write: 0,
                }),
                stop_reason: Some(StopReason::ToolUse),
                error: None,
                model: Some("local".into()),
            },
            EntryBody::ToolResult {
                tool_call_id: "c1".into(),
                tool_name: "bash".into(),
                content: vec![Block::Text { text: "out".into() }],
                is_error: true,
            },
            EntryBody::Custom {
                custom_type: "k".into(),
                data: json!({"a": 1}),
            },
            EntryBody::CustomMessage {
                custom_type: "k".into(),
                content: vec![Block::Text { text: "m".into() }],
            },
            EntryBody::Compaction {
                summary: "s".into(),
                first_kept_id: "00000001".into(),
                tokens_before: Some(9),
            },
            EntryBody::Label {
                target_id: "00000001".into(),
                label: Some("l".into()),
            },
            EntryBody::Checkpoint {
                commit: "abc".into(),
            },
            EntryBody::PolicyDecision {
                tool_call_id: "c1".into(),
                decision: "ask".into(),
                reason: "git push".into(),
                rewrite: None,
            },
            EntryBody::GuardEvent {
                guard: "repeat".into(),
                message: "m".into(),
                tool_call_id: Some("c1".into()),
            },
        ];
        let mut ids = IdGen::new();
        for body in bodies {
            let e = ids.push(1, body);
            let line = serde_json::to_string(&e).unwrap();
            let back: Entry = serde_json::from_str(&line).unwrap();
            assert_eq!(back, e, "{line}");
        }
    }

    #[test]
    fn model_step_ignores_empty_assistant_messages() {
        let mut ids = IdGen::new();
        let empty = ids.push(
            0,
            EntryBody::Message {
                role: Role::Assistant,
                content: vec![],
                usage: None,
                stop_reason: Some(StopReason::Aborted),
                error: Some("aborted".into()),
                model: None,
            },
        );
        assert!(!empty.is_model_step());
    }
}
