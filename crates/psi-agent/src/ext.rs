//! Extension tools and events in the agent (psi-plan.md §9.4): tools
//! registered from Lua or as script tools go through the same shape fixes
//! and policy as core tools.

use std::sync::{Arc, Weak};

use psi_ai::{Param, ParamKind, ToolSchema};
use psi_ext::{ExtManager, ToolInfo};
use psi_safety::checkpoint::Checkpoints;
use psi_safety::policy::{Decision, FileFacts, Policy};
use psi_tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{Map, Value, json};

use crate::approval::{Approval, ApprovalRequest, Approver};

/// What a call runs: a core tool or an extension's.
#[derive(Debug, Clone)]
pub enum Callable {
    Core(Tool),
    Ext(ToolInfo),
}

impl Callable {
    pub fn name(&self) -> &str {
        match self {
            Callable::Core(t) => t.name(),
            Callable::Ext(i) => &i.name,
        }
    }

    pub fn read_only(&self) -> bool {
        match self {
            Callable::Core(t) => t.read_only(),
            Callable::Ext(i) => i.read_only,
        }
    }
}

/// Schema names and texts are `&'static str`; extension tools are few and
/// built once per load, so their strings are leaked.
fn leak(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

pub fn schema_of(info: &ToolInfo) -> ToolSchema {
    let mut example = Map::new();
    let params = info
        .params
        .iter()
        .map(|(name, ty, desc, required)| {
            let kind = ParamKind::parse(ty).unwrap_or(ParamKind::String);
            if *required {
                let v = match kind {
                    ParamKind::Integer | ParamKind::Number => json!(1),
                    ParamKind::Boolean => json!(true),
                    ParamKind::Array => json!([]),
                    ParamKind::Object => json!({}),
                    ParamKind::String => json!("..."),
                };
                example.insert(name.clone(), v);
            }
            let p = Param::new(leak(name), kind, leak(desc));
            if *required { p } else { p.optional() }
        })
        .collect();
    ToolSchema {
        name: leak(&info.name),
        description: info.description.clone(),
        params,
        example: Value::Object(example),
    }
}

/// Runs a call that the policy allowed.
pub async fn invoke(
    c: &Callable,
    args: &Map<String, Value>,
    ctx: &ToolCtx,
    ext: &ExtManager,
) -> ToolOutput {
    match c {
        Callable::Core(t) => t.run(args, ctx).await,
        Callable::Ext(info) => {
            if ctx.cancel.is_cancelled() {
                return ToolOutput::err("Aborted before it ran.");
            }
            let args = Value::Object(args.clone());
            if let Some(script) = ext.script(&info.name) {
                return psi_tools::bash::run_script(&script.argv(), &args, ctx).await;
            }
            let (text, is_error) = tokio::select! {
                r = ext.call_tool(&info.name, args) => r,
                () = ctx.cancel.cancelled() => ("[aborted]".into(), true),
            };
            ToolOutput { text, is_error }
        }
    }
}

/// What the policy says about a call that may change something (§8.1).
pub async fn decide(
    policy: &Policy,
    c: &Callable,
    args: &Map<String, Value>,
    ctx: &ToolCtx,
    checkpoints: Option<Arc<Checkpoints>>,
    capture_failing: bool,
    ext: &ExtManager,
) -> Decision {
    match c {
        Callable::Core(Tool::Bash) => {
            policy.bash(args.get("command").and_then(Value::as_str).unwrap_or(""))
        }
        Callable::Core(t @ (Tool::Write | Tool::Edit)) => {
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
        Callable::Core(_) => Decision::Allow,
        Callable::Ext(info) => {
            let script = ext.script(&info.name).is_some();
            policy.ext_tool(&info.name, info.read_only, info.always_ask, script)
        }
    }
}

/// Resolves a tool name for `psi.execute_tool`.
fn resolve(name: &str, core: &[Tool], ext: &ExtManager) -> Option<Callable> {
    if let Some(t) = Tool::from_name(name).filter(|t| core.contains(t)) {
        return Some(Callable::Core(t));
    }
    ext.tools()
        .into_iter()
        .find(|t| t.name == name)
        .map(Callable::Ext)
}

/// `psi.execute_tool`: a tool run for an extension, through the policy.
/// Asks go to the approver; nothing is recorded in the session.
pub fn tool_runner(
    ctx: ToolCtx,
    policy: Policy,
    approver: Arc<dyn Approver>,
    core: Vec<Tool>,
    ext: Weak<ExtManager>,
) -> psi_ext::ToolRunner {
    Arc::new(move |name: String, args: Value| {
        let (ctx, policy, approver, core, ext) = (
            ctx.clone(),
            policy.clone(),
            approver.clone(),
            core.clone(),
            ext.clone(),
        );
        Box::pin(async move {
            let Some(ext) = ext.upgrade() else {
                return ("psi is shutting down".into(), true);
            };
            let Value::Object(args) = args else {
                return ("execute_tool: args must be a table".into(), true);
            };
            let Some(c) = resolve(&name, &core, &ext) else {
                return (format!("There is no tool named '{name}'."), true);
            };
            match decide(&policy, &c, &args, &ctx, None, false, &ext).await {
                Decision::Allow => {}
                Decision::Refuse(m) => return (m, true),
                Decision::Ask { reason, keys } => {
                    let req = ApprovalRequest {
                        tool_call_id: String::new(),
                        tool: name.clone(),
                        args: Value::Object(args.clone()),
                        intent: Some("an extension runs it".into()),
                        preceding_text: String::new(),
                        reason: reason.clone(),
                        keys,
                    };
                    match approver.approve(req).await {
                        Approval::Once | Approval::Session => {}
                        _ => return (format!("Not run: {reason}, and it was denied."), true),
                    }
                }
            }
            let o = invoke(&c, &args, &ctx, &ext).await;
            (o.text, o.is_error)
        })
    })
}
