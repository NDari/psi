//! What the policy says about a tool call.

use std::sync::Arc;

use psi_safety::checkpoint::Checkpoints;
use psi_safety::policy::{Decision, FileFacts, Policy};
use psi_tools::{Tool, ToolCtx};
use serde_json::{Map, Value};

/// What the policy says about a call that may change something (§8.1).
pub async fn decide(
    policy: &Policy,
    tool: &Tool,
    args: &Map<String, Value>,
    ctx: &ToolCtx,
    checkpoints: Option<Arc<Checkpoints>>,
    capture_failing: bool,
) -> Decision {
    match tool {
        Tool::Bash => policy.bash(args.get("command").and_then(Value::as_str).unwrap_or("")),
        t @ (Tool::Write | Tool::Edit) => {
            let path = t.target(args, ctx).unwrap_or_default();
            let ignored = match checkpoints {
                Some(cp) => {
                    let p = path.clone();
                    tokio::task::spawn_blocking(move || cp.is_ignored(&p))
                        .await
                        .unwrap_or(false)
                }
                None => false,
            };
            let facts = FileFacts {
                ignored,
                capture_failing,
            };
            let shown = args.get("path").and_then(Value::as_str).unwrap_or("");
            policy.file_write(t.name(), &path, shown, facts)
        }
        _ => Decision::Allow,
    }
}
