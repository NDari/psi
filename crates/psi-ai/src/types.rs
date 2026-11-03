//! Chat requests and responses in the OpenAI-compatible shape llama-server
//! speaks.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One message of the conversation as sent to the model.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatMessage {
    System(String),
    User(String),
    Assistant {
        text: String,
        tool_calls: Vec<RawToolCall>,
    },
    Tool {
        call_id: String,
        content: String,
    },
}

impl ChatMessage {
    /// The message in OpenAI chat format, as sent.
    pub fn to_json(&self) -> Value {
        match self {
            ChatMessage::System(t) => json!({"role": "system", "content": t}),
            ChatMessage::User(t) => json!({"role": "user", "content": t}),
            ChatMessage::Assistant { text, tool_calls } if tool_calls.is_empty() => {
                json!({"role": "assistant", "content": text})
            }
            ChatMessage::Assistant { text, tool_calls } => json!({
                "role": "assistant",
                "content": text,
                "tool_calls": tool_calls.iter().map(|c| json!({
                    "id": c.id,
                    "type": "function",
                    "function": {"name": c.name, "arguments": c.arguments},
                })).collect::<Vec<_>>(),
            }),
            ChatMessage::Tool { call_id, content } => {
                json!({"role": "tool", "tool_call_id": call_id, "content": content})
            }
        }
    }
}

/// A tool call as the model sent it: arguments are the raw JSON text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// Each tool's JSON schema (`{"type": "function", ...}`).
    pub tools: Vec<Value>,
    pub max_tokens: u64,
    /// Sent as `chat_template_kwargs.enable_thinking` when set.
    pub enable_thinking: Option<bool>,
}

impl ChatRequest {
    /// The exact bytes sent. No sampling fields: the server's settings apply.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.body(false)
    }

    /// The bytes, asking llama-server to stream prompt-processing progress
    /// when `progress` (other servers may refuse the field).
    pub fn body(&self, progress: bool) -> Vec<u8> {
        let mut body = json!({
            "model": self.model,
            "messages": self.messages.iter().map(ChatMessage::to_json).collect::<Vec<_>>(),
            "stream": true,
            "stream_options": {"include_usage": true},
            "max_tokens": self.max_tokens,
        });
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(self.tools.clone());
        }
        if let Some(t) = self.enable_thinking {
            body["chat_template_kwargs"] = json!({"enable_thinking": t});
        }
        if progress {
            body["return_progress"] = json!(true);
        }
        // A Value always serializes.
        serde_json::to_vec(&body).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    /// The output limit cut the response off.
    Length,
    /// The stream ended without a finish reason.
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Prompt tokens processed now (not from the cache).
    pub input: u64,
    pub output: u64,
    /// Prompt tokens served from the server's cache.
    pub cache_read: u64,
}

impl TokenUsage {
    pub fn prompt_total(&self) -> u64 {
        self.input + self.cache_read
    }
}

/// llama-server's `timings`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Timings {
    pub prompt_ms: f64,
    pub predicted_ms: f64,
    pub predicted_per_second: f64,
    /// Prompt tokens processed per second (the ones not from the cache).
    #[serde(default)]
    pub prompt_per_second: f64,
    /// Prompt tokens processed now, and reused from the cache.
    #[serde(default)]
    pub prompt_n: u64,
    #[serde(default)]
    pub cache_n: u64,
}

/// A finished model response.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub text: String,
    pub reasoning: String,
    pub tool_calls: Vec<RawToolCall>,
    pub finish: FinishReason,
    pub usage: Option<TokenUsage>,
    pub timings: Option<Timings>,
    /// Milliseconds from sending the request to the first streamed token.
    pub first_token_ms: Option<u64>,
}

/// Streamed pieces, for printing as they arrive.
#[derive(Debug, Clone, PartialEq)]
pub enum Delta {
    Text(String),
    Reasoning(String),
    ToolCallStart {
        index: usize,
        name: String,
    },
    /// llama-server reading the prompt (`return_progress`): tokens done of
    /// `total`, `cache` of them reused.
    PromptProgress {
        total: u64,
        cache: u64,
        processed: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_has_no_sampling_and_flat_messages() {
        let req = ChatRequest {
            model: "local".into(),
            messages: vec![
                ChatMessage::System("s".into()),
                ChatMessage::User("u".into()),
                ChatMessage::Assistant {
                    text: String::new(),
                    tool_calls: vec![RawToolCall {
                        id: "c1".into(),
                        name: "read".into(),
                        arguments: "{\"path\":\"a\"}".into(),
                    }],
                },
                ChatMessage::Tool {
                    call_id: "c1".into(),
                    content: "x".into(),
                },
            ],
            tools: vec![],
            max_tokens: 10,
            enable_thinking: None,
        };
        let v: Value = serde_json::from_slice(&req.to_bytes()).unwrap();
        assert!(v.get("temperature").is_none());
        assert!(v.get("tools").is_none());
        assert_eq!(
            v["messages"][2]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a\"}"
        );
        assert_eq!(
            v["messages"][3],
            json!({"role": "tool", "tool_call_id": "c1", "content": "x"})
        );
        assert_eq!(v["stream_options"]["include_usage"], true);
        assert!(v.get("return_progress").is_none());
        let with: Value = serde_json::from_slice(&req.body(true)).unwrap();
        assert_eq!(with["return_progress"], true);
    }
}
