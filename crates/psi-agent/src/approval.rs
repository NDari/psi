//! How the agent asks the user (psi-plan.md §8.7). The TUI answers with a
//! card; headless runs never prompt and deny (D-18).

use std::path::PathBuf;

use futures_util::future::BoxFuture;
use serde::Serialize;
use serde_json::Value;

/// One call that needs the user's yes.
#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequest {
    pub tool_call_id: String,
    pub tool: String,
    pub args: Value,
    /// The model's optional one-line intent (D-19).
    pub intent: Option<String>,
    /// The model's text before the call, shown when there is no intent.
    pub preceding_text: String,
    /// Why it asks.
    pub reason: String,
    /// What a session grant covers.
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    Once,
    /// Allow this and later calls with the same keys this session.
    Session,
    Deny,
    /// Deny, with a message for the model.
    DenyWith(String),
}

/// A write the boundary blocked (§8.5): the harness offers the fix.
#[derive(Debug, Clone, Serialize)]
pub struct BlockedRequest {
    pub tool_call_id: String,
    pub command: String,
    /// The path the write was blocked on.
    pub path: PathBuf,
    /// The directory "add for the session" would make writable.
    pub dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedChoice {
    /// Run it once more, outside the boundary.
    RerunOutside,
    /// Make `dir` writable for the session and run it again inside.
    AddDir,
    No,
}

pub trait Approver: Send + Sync {
    fn approve(&self, req: ApprovalRequest) -> BoxFuture<'static, Approval>;
    fn blocked(&self, req: BlockedRequest) -> BoxFuture<'static, BlockedChoice>;
    /// A guard paused the run; true goes on.
    fn pause(&self, reason: String) -> BoxFuture<'static, bool>;
    /// Whether a person answers; headless runs say so in denials.
    fn interactive(&self) -> bool;
}

/// Print, JSON and eval runs: nothing prompts, anything that would ask is
/// denied (`--allow` grants list entries up front), and a pause stops.
#[derive(Debug, Clone, Copy, Default)]
pub struct Headless;

impl Approver for Headless {
    fn approve(&self, _: ApprovalRequest) -> BoxFuture<'static, Approval> {
        Box::pin(async { Approval::Deny })
    }
    fn blocked(&self, _: BlockedRequest) -> BoxFuture<'static, BlockedChoice> {
        Box::pin(async { BlockedChoice::No })
    }
    fn pause(&self, _: String) -> BoxFuture<'static, bool> {
        Box::pin(async { false })
    }
    fn interactive(&self) -> bool {
        false
    }
}
