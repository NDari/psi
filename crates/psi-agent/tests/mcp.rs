#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! MCP tools through the real agent loop (psi-plan.md §10.2, D-42): a fake
//! server over an in-process pipe, the replay server as the model.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use psi_agent::approval::{Approval, ApprovalRequest, Approver, BlockedChoice, BlockedRequest};
use psi_agent::{Agent, AgentConfig, Budgets, Safety, Stop};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_mcp::testing::{config, fake_manager};
use psi_mcp::{Approval as McpApproval, McpManager};
use psi_safety::always_ask::Rules;
use psi_safety::policy::{Mode, Policy};
use psi_session::{EntryBody, SessionFile, text_of};
use psi_tools::{ToolCtx, core_tools};
use serde_json::{Map, Value, json};
use tempfile::TempDir;

#[derive(Default)]
struct Scripted {
    approvals: Mutex<VecDeque<Approval>>,
    asked: Mutex<Vec<ApprovalRequest>>,
}

impl Approver for Scripted {
    fn approve(&self, req: ApprovalRequest) -> BoxFuture<'static, Approval> {
        self.asked.lock().unwrap().push(req);
        let a = self
            .approvals
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Approval::Deny);
        Box::pin(async move { a })
    }
    fn blocked(&self, _: BlockedRequest) -> BoxFuture<'static, BlockedChoice> {
        Box::pin(async { BlockedChoice::No })
    }
    fn pause(&self, _: String) -> BoxFuture<'static, bool> {
        Box::pin(async { false })
    }
    fn interactive(&self) -> bool {
        true
    }
}

fn tools() -> Value {
    json!([
        {"name": "search", "description": "Finds issues.", "annotations": {"readOnlyHint": true},
         "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}, "labels": {"type": "array", "items": {"type": "string"}}}, "required": ["q"]}},
        {"name": "close", "description": "Closes an issue.",
         "inputSchema": {"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]}}
    ])
}

fn answer(tool: &str, args: &Map<String, Value>) -> Value {
    let text = match tool {
        "search" => format!("issue 7 matches {}", args["q"]),
        _ => format!("closed {}", args["id"]),
    };
    json!({"content": [{"type": "text", "text": text}]})
}

struct Fx {
    _tmp: TempDir,
    root: PathBuf,
    server: ReplayServer,
    approver: Arc<Scripted>,
}

impl Fx {
    fn new(replies: Vec<Reply>) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        Self {
            root,
            server: ReplayServer::start(replies).unwrap(),
            approver: Arc::new(Scripted::default()),
            _tmp: tmp,
        }
    }

    fn agent(&self, mcp: Arc<McpManager>) -> Agent {
        let home = self.root.join("../home");
        let session = SessionFile::create(&home.join(".psi/sessions"), &self.root).unwrap();
        let tmpdir = home.join("tmp");
        let safety = Safety {
            policy: Policy::new(
                Mode::Auto,
                Rules::shipped(),
                &self.root,
                Some(&home),
                &tmpdir,
            ),
            checkpoints: None,
            approver: self.approver.clone(),
            grants: HashSet::new(),
            info: Value::Null,
        };
        let cfg = AgentConfig {
            model: "local".into(),
            max_output: 512,
            n_ctx: 32_768,
            enable_thinking: None,
            budgets: Budgets::default(),
            system_prompt: "sys\nTools: read\nmore".into(),
            tools: core_tools(false),
            home: Some(home),
        };
        let ctx = ToolCtx::new(&self.root, &tmpdir, 20_000);
        Agent::new(Client::new(&self.server.base_url()), session, cfg, ctx)
            .with_safety(safety)
            .with_mcp(mcp)
    }
}

fn call(name: &str, args: Value) -> Reply {
    Reply::Chat(ScriptedReply::call(name, args))
}

fn text(t: &str) -> Reply {
    Reply::Chat(ScriptedReply::text(t))
}

fn results(a: &Agent) -> Vec<(String, bool)> {
    a.session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::ToolResult {
                content, is_error, ..
            } => Some((text_of(content), *is_error)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn read_only_tools_run_and_others_ask() {
    let fx = Fx::new(vec![
        call("mcp__gh__search", json!({"q": "crash"})),
        call("mcp__gh__close", json!({"id": "7"})),
        call("mcp__gh__close", json!({"id": 7})),
        text("Closed issue 7."),
    ]);
    fx.approver
        .approvals
        .lock()
        .unwrap()
        .extend([Approval::Deny, Approval::Once]);
    let mcp = fake_manager(config("gh"), tools(), Arc::new(answer)).await;
    let mut a = fx.agent(mcp);
    let out = a.run("close the crash issue").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);

    // Declared to the model like core tools, with the server's schema.
    let reqs = fx.server.chat_requests();
    let declared: Vec<&Value> = reqs[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["function"]["name"].as_str().unwrap().starts_with("mcp__"))
        .collect();
    assert_eq!(declared.len(), 2);
    assert_eq!(declared[0]["function"]["name"], "mcp__gh__search");
    // The prompt's tool line names them too, not only the core tools.
    let system = reqs[0]["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains(
            "\nTools: read, write, edit, bash; from MCP servers: mcp__gh__search, mcp__gh__close\nmore"
        ),
        "{system}"
    );
    assert_eq!(
        declared[0]["function"]["parameters"]["properties"]["labels"],
        json!({"type": "array", "items": {"type": "string"}})
    );

    let r = results(&a);
    assert_eq!(r[0], ("issue 7 matches \"crash\"".into(), false));
    // The string "7" was coerced to the integer the schema asks for; the
    // call asked first and was denied, then approved.
    assert!(r[1].1 && r[1].0.contains("denied"), "{:?}", r[1]);
    assert_eq!(r[2], ("closed 7".into(), false));
    let asked = fx.approver.asked.lock().unwrap();
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[0].tool, "mcp__gh__close");
    assert!(asked[0].reason.contains("MCP tool"), "{}", asked[0].reason);
}

#[tokio::test]
async fn approval_settings_and_plan_mode() {
    let fx = Fx::new(vec![
        call("mcp__gh__close", json!({"id": 1})),
        text("ok"),
        call("mcp__gh__close", json!({"id": 2})),
        text("ok"),
    ]);
    let mut c = config("gh");
    c.approval = McpApproval::Never;
    let mcp = fake_manager(c, tools(), Arc::new(answer)).await;
    let mut a = fx.agent(mcp);
    a.run("close 1").await;
    assert_eq!(results(&a)[0], ("closed 1".into(), false));
    assert!(fx.approver.asked.lock().unwrap().is_empty());
    // Plan mode allows only tools the server marks read-only.
    a.set_plan_mode(true).unwrap();
    a.run("close 2").await;
    let r = &results(&a)[1];
    assert!(
        r.1 && r.0.starts_with("Not run: mcp__gh__close may change things"),
        "{r:?}"
    );
}

#[tokio::test]
async fn a_server_that_is_gone_says_so() {
    let fx = Fx::new(vec![call("mcp__gh__search", json!({"q": "x"})), text("ok")]);
    let mcp = McpManager::start(
        vec![{
            let mut c = config("gh");
            c.transport = psi_mcp::Transport::Stdio {
                command: "/nonexistent/server".into(),
                args: vec![],
                env: vec![],
                cwd: None,
            };
            c
        }],
        vec![],
        Path::new("."),
    );
    let mut a = fx.agent(mcp);
    a.run("search").await;
    // No tools offered, so the call names a tool that doesn't exist.
    let r = &results(&a)[0];
    assert!(
        r.1 && r.0.starts_with("There is no tool named 'mcp__gh__search'"),
        "{r:?}"
    );
}
