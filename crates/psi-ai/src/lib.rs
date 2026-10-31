//! Providers, streaming, tool-call shape fixes, model profiles and the wire
//! log (psi-plan.md §4).

pub mod client;
pub mod profile;
pub mod replay;
pub mod schema;
pub mod shapefix;
mod sse;
mod stream;
pub mod types;
pub mod wire;

pub use client::{AiError, Client, Props};
pub use profile::ModelProfile;
pub use schema::{Param, ParamKind, ToolSchema};
pub use shapefix::{FixedCall, fix_call};
pub use types::{
    ChatMessage, ChatRequest, Delta, FinishReason, RawToolCall, Response, Timings, TokenUsage,
};
pub use wire::WireLog;

// TODO(phase-7): Anthropic Messages, OpenAI Responses and remote providers
// over TLS; providers registered from Lua in phase 6.
