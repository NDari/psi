//! What a run reports as it goes (JSON mode prints these; extensions and
//! the TUI will listen to the same stream).

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
    /// Streamed answer text.
    TextDelta {
        text: String,
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
    /// What a sub-agent is doing (§10.1): its tool calls, as they start.
    Subagent {
        agent: String,
        event: Box<AgentEvent>,
    },
    /// A summary of the earlier conversation is being written (§7.4).
    Compacting {
        tokens: u64,
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
