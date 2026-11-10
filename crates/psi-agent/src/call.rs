//! What the policy says about a tool call.

use psi_safety::policy::{Decision, Policy};
use psi_tools::{Tool, ToolCtx};
use serde_json::{Map, Value};

/// Whether a call runs or is refused (`psi_safety::policy`).
pub fn decide(policy: &Policy, tool: &Tool, args: &Map<String, Value>, ctx: &ToolCtx) -> Decision {
    let shown = args.get("path").and_then(Value::as_str).unwrap_or("");
    match tool {
        Tool::Bash => policy.bash(args.get("command").and_then(Value::as_str).unwrap_or("")),
        Tool::Read => policy.file_read(&ctx.resolve(shown), shown),
        Tool::Write | Tool::Edit => policy.file_write(&ctx.resolve(shown), shown),
    }
}
