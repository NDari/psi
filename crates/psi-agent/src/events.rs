//! What a run reports as it goes (JSON mode prints these; the TUI
//! listens to the same stream).

use psi_session::Entry;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    RunStart {
        session: String,
    },
    TurnStart {
        turn: u32,
    },
    /// The server is reading the prompt: `processed` of `total` tokens,
    /// `cache` of them reused (llama-server's `return_progress`).
    PromptProgress {
        total: u64,
        cache: u64,
        processed: u64,
    },
    /// Streamed answer text.
    TextDelta {
        text: String,
    },
    /// The model is thinking: the characters of reasoning so far in this
    /// response. Sent about every 64 characters, for a live display; the
    /// reasoning itself is in the response's session entry.
    Thinking {
        chars: u64,
    },
    /// The model is writing a tool call (the `index`-th of its response):
    /// the characters of arguments so far, and the `path` argument once it
    /// is whole. Sent when the call starts and about every 64 characters;
    /// `ToolStart` follows when the response has ended and the call runs.
    CallProgress {
        index: usize,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        chars: u64,
    },
    /// Every entry appended to the session, as written.
    Entry {
        entry: Entry,
    },
    ToolStart {
        id: String,
        name: String,
        args: Value,
    },
    ToolEnd {
        id: String,
        name: String,
        is_error: bool,
    },
    TurnEnd {
        turn: u32,
    },
    /// A summary of the earlier conversation is being written (§7.4).
    Compacting {
        tokens: u64,
    },
    /// The summary that `Compacting` announced is done, failed or was
    /// stopped; `done` when the conversation was summarized.
    Compacted {
        done: bool,
    },
    /// A request failed in a way that may pass, and is sent once more. Text
    /// streamed for the failed attempt is void.
    Retrying {
        reason: String,
    },
    /// Something the user should know that isn't part of the conversation.
    Notice {
        text: String,
    },
    RunEnd {
        stop: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        text: String,
        turns: u32,
        tool_calls: u32,
    },
}
