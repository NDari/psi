#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! Sub-agents through the real agent loop (psi-plan.md §10.1, D-41): the
//! replay server plays the parent and the sub-agent in turn (one slot), and
//! a scripted approver records what was asked.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use psi_agent::approval::{Approval, ApprovalRequest, Approver, BlockedChoice, BlockedRequest};
use psi_agent::subagent::{SUB_NOTE, built_in};
use psi_agent::{Agent, AgentConfig, Budgets, Safety, Stop};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_safety::always_ask::Rules;
use psi_safety::checkpoint::Checkpoints;
use psi_safety::policy::{Mode, Policy};
use psi_session::{EntryBody, SessionFile, text_of};
use psi_tools::{ToolCtx, core_tools};
use serde_json::{Value, json};
use tempfile::TempDir;

/// Approves in order; records what it was asked.
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

struct Fx {
    _tmp: TempDir,
    root: PathBuf,
    home: PathBuf,
    sessions: PathBuf,
    server: ReplayServer,
    approver: Arc<Scripted>,
}

impl Fx {
    fn new(replies: Vec<Reply>) -> Self {
        let t = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let tmp = tempfile::tempdir_in(t).unwrap();
        let root = tmp.path().join("project");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(home.join(".psi")).unwrap();
        Self {
            sessions: home.join(".psi/sessions"),
            root,
            home,
            server: ReplayServer::start(replies).unwrap(),
            approver: Arc::new(Scripted::default()),
            _tmp: tmp,
        }
    }

    /// An agent with sub-agents on, and checkpoints when `cp`.
    fn agent(&self, cp: bool) -> Agent {
        self.agent_with(cp, Budgets::default())
    }

    fn agent_with(&self, cp: bool, budgets: Budgets) -> Agent {
        let session = SessionFile::create(&self.sessions, &self.root).unwrap();
        let tmpdir = self.home.join(".cache/psi/tmp/s");
        let checkpoints = cp.then(|| {
            let id = session.path().file_stem().unwrap().to_string_lossy();
            Arc::new(
                Checkpoints::open(&self.home.join(".psi/checkpoints"), &self.root, &id).unwrap(),
            )
        });
        let safety = Safety {
            policy: Policy::new(
                Mode::Auto,
                Rules::shipped(),
                &self.root,
                Some(&self.home),
                &tmpdir,
            ),
            checkpoints,
            approver: self.approver.clone(),
            grants: HashSet::new(),
            info: Value::Null,
        };
        let cfg = AgentConfig {
            model: "local".into(),
            max_output: 512,
            n_ctx: 32_768,
            enable_thinking: None,
            budgets,
            system_prompt: "sys".into(),
            tools: core_tools(false),
            home: Some(self.home.clone()),
        };
        let ctx = ToolCtx::new(&self.root, &tmpdir, 20_000);
        Agent::new(Client::new(&self.server.base_url()), session, cfg, ctx)
            .with_safety(safety)
            .with_subagents(built_in())
    }
}

fn call(name: &str, args: Value) -> Reply {
    Reply::Chat(ScriptedReply::call(name, args))
}

fn text(t: &str) -> Reply {
    Reply::Chat(ScriptedReply::text(t))
}

fn tool_names(req: &Value) -> Vec<String> {
    req["tools"]
        .as_array()
        .map(|t| {
            t.iter()
                .map(|t| t["function"]["name"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
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

fn subagent_info(a: &Agent) -> Vec<Value> {
    a.session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::Custom { custom_type, data } if custom_type == "subagent" => {
                Some(data.clone())
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_scout_works_in_its_own_context_and_reports_back() {
    let fx = Fx::new(vec![
        call(
            "agent",
            json!({"agent": "scout", "task": "Find what a.txt says."}),
        ),
        call("read", json!({"path": "a.txt"})),
        text("a.txt says hello."),
        text("It says hello."),
    ]);
    std::fs::write(fx.root.join("a.txt"), "hello\n").unwrap();
    let mut a = fx.agent(true);
    let out = a.run("what does a.txt say?").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);

    let reqs = fx.server.chat_requests();
    assert_eq!(reqs.len(), 4);
    assert!(tool_names(&reqs[0]).contains(&"agent".to_owned()));
    // The sub-agent sees its task only, with the note and its prompt, and
    // gets only its tools: no agent tool, no write or edit.
    let sub = reqs[1]["messages"].as_array().unwrap();
    assert_eq!(sub.len(), 2);
    let system = sub[0]["content"].as_str().unwrap();
    assert!(system.starts_with("sys\n\n") && system.contains(SUB_NOTE));
    assert!(system.contains("You are scout."));
    assert_eq!(sub[1]["content"], "Find what a.txt says.");
    assert_eq!(tool_names(&reqs[1]), ["read", "bash"]);

    // Its last answer comes back, with how it went.
    let (r, is_error) = &results(&a)[0];
    assert!(!is_error);
    assert_eq!(
        r,
        "a.txt says hello.\n\n[scout: 2 model requests, 1 tool calls; done]"
    );
    // Its session is its own file, out of /resume's list.
    let info = &subagent_info(&a)[0];
    let path = PathBuf::from(info["session"].as_str().unwrap());
    assert!(path.ends_with("01-scout.jsonl"), "{path:?}");
    assert!(
        path.parent()
            .unwrap()
            .to_string_lossy()
            .ends_with(".agents")
    );
    let child = SessionFile::open(&path).unwrap();
    assert!(child.entries().iter().any(|e| e.is_model_step()));
    assert!(std::fs::metadata(child.wire_path()).unwrap().len() > 0);
    let dir = a.session().path().parent().unwrap().to_owned();
    assert_eq!(SessionFile::list(&dir).len(), 1);
    assert_eq!(info["turns"], 2);
    // A read-only agent's call takes no checkpoint.
    assert!(!a.session().entries().iter().any(|e| matches!(
        &e.body,
        EntryBody::Checkpoint { trigger: Some(t), .. } if t == "tool"
    )));
}

#[tokio::test]
async fn a_read_only_agent_runs_only_read_only_commands() {
    let fx = Fx::new(vec![
        call(
            "agent",
            json!({"agent": "scout", "task": "Clean up a.txt."}),
        ),
        call("bash", json!({"command": "rm a.txt"})),
        text("I can't remove files."),
        text("ok"),
    ]);
    std::fs::write(fx.root.join("a.txt"), "x").unwrap();
    let mut a = fx.agent(false);
    a.run("clean up").await;
    assert!(fx.root.join("a.txt").exists());
    let reqs = fx.server.chat_requests();
    let refusal = reqs[2]["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        refusal.starts_with("Not run: scout is a read-only agent"),
        "{refusal}"
    );
}

#[tokio::test]
async fn a_worker_asks_by_name_and_its_changes_are_checkpointed() {
    let fx = Fx::new(vec![
        call(
            "agent",
            json!({"agent": "worker", "task": "Push and add b.txt."}),
        ),
        call("bash", json!({"command": "git push"})),
        call("write", json!({"path": "b.txt", "text": "b\n"})),
        text("Pushed and wrote b.txt."),
        text("done"),
    ]);
    fx.approver
        .approvals
        .lock()
        .unwrap()
        .push_back(Approval::Deny);
    let mut a = fx.agent(true);
    let out = a.run("go").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    let asked = fx.approver.asked.lock().unwrap();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].agent.as_deref(), Some("worker"));
    assert_eq!(asked[0].tool, "bash");
    assert_eq!(
        std::fs::read_to_string(fx.root.join("b.txt")).unwrap(),
        "b\n"
    );
    // The parent checkpoints after the call, which changed files.
    let entries = a.session().entries();
    let sub = entries
        .iter()
        .position(|e| matches!(&e.body, EntryBody::Custom { custom_type, .. } if custom_type == "subagent"))
        .unwrap();
    assert!(entries[sub..].iter().any(|e| matches!(
        &e.body,
        EntryBody::Checkpoint { trigger: Some(t), .. } if t == "tool"
    )));
}

#[tokio::test]
async fn unknown_agents_and_plan_mode_are_refused() {
    let fx = Fx::new(vec![
        call("agent", json!({"agent": "nope", "task": "x"})),
        call("agent", json!({"agent": "worker", "task": "x"})),
        text("ok"),
    ]);
    let mut a = fx.agent(false);
    a.set_plan_mode(true).unwrap();
    a.run("plan it").await;
    let r = results(&a);
    assert_eq!(
        r[0],
        (
            "There is no agent named 'nope'. Agents: scout, worker.".to_owned(),
            true
        )
    );
    assert!(
        r[1].0
            .starts_with("Not run: worker may change files, and plan mode"),
        "{}",
        r[1].0
    );
    assert_eq!(fx.server.chat_requests().len(), 3);
}

#[tokio::test]
async fn the_agent_tool_is_off_unless_sub_agents_are_set() {
    let fx = Fx::new(vec![text("hi")]);
    let session = SessionFile::create(&fx.sessions, &fx.root).unwrap();
    let cfg = AgentConfig {
        model: "local".into(),
        max_output: 512,
        n_ctx: 32_768,
        enable_thinking: None,
        budgets: Budgets::default(),
        system_prompt: "sys".into(),
        tools: core_tools(false),
        home: None,
    };
    let ctx = ToolCtx::new(&fx.root, &fx.root, 20_000);
    let mut a = Agent::new(Client::new(&fx.server.base_url()), session, cfg, ctx);
    a.run("hi").await;
    let reqs = fx.server.chat_requests();
    assert!(!tool_names(&reqs[0]).contains(&"agent".to_owned()));
}

#[tokio::test]
async fn reads_held_back_for_room_point_to_scout() {
    let fx = Fx::new(vec![
        Reply::Chat(ScriptedReply {
            tool_calls: (0..4)
                .map(|i| {
                    (
                        "read".into(),
                        json!({"path": format!("f{i}.txt")}).to_string(),
                    )
                })
                .collect(),
            ..Default::default()
        }),
        text("ok"),
    ]);
    for i in 0..4 {
        std::fs::write(
            fx.root.join(format!("f{i}.txt")),
            format!("{}\n", "x".repeat(99)).repeat(500),
        )
        .unwrap();
    }
    let mut a = fx.agent(false);
    a.run("read them").await;
    let r = results(&a);
    let held = &r.last().unwrap().0;
    assert!(held.starts_with("Not run:"), "{held}");
    assert!(
        held.ends_with("give the reading to scout with the agent tool."),
        "{held}"
    );
}

#[tokio::test]
async fn in_the_tui_a_sub_agent_has_no_request_limit() {
    // The TUI has no limit for the main run; a sub-agent used to pause at
    // 100 requests, and worker paused six times in the cog trial waiting on
    // the user (2026-11-04). It now has the parent's budgets.
    let mut replies = vec![call(
        "agent",
        json!({"agent": "worker", "task": "Run the steps."}),
    )];
    for i in 0..110 {
        replies.push(call("bash", json!({"command": format!("echo {i}")})));
    }
    replies.push(text("Ran 110 steps."));
    replies.push(text("Done."));
    let fx = Fx::new(replies);
    let tui = Budgets {
        max_turns: u32::MAX,
        max_tool_calls: u32::MAX,
        ..Budgets::default()
    };
    let mut a = fx.agent_with(false, tui);
    let out = a.run("go").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    let info = subagent_info(&a);
    assert_eq!(info[0]["turns"], 111, "{info:?}");
    assert_eq!(info[0]["stop"], "done", "{info:?}");
}

/// Scout stopped by a 3-request budget (the pause answers stop), then
/// `report` as the reply to the request for its last report.
fn stopped_scout(report: Reply) -> (Fx, Agent) {
    let mut replies = vec![call(
        "agent",
        json!({"agent": "scout", "task": "Find the config."}),
    )];
    for _ in 0..3 {
        replies.push(call("bash", json!({"command": "ls"})));
    }
    replies.push(report);
    replies.push(text("Done."));
    let fx = Fx::new(replies);
    let three = Budgets {
        max_turns: 3,
        ..Budgets::default()
    };
    let a = fx.agent_with(false, three);
    (fx, a)
}

#[tokio::test]
async fn a_stopped_sub_agent_is_asked_for_what_it_found() {
    // Scout 02 in the cog trial stopped at a guard after 191 requests and
    // returned "(no answer)"; the parent read everything again itself.
    let (fx, mut a) = stopped_scout(text("The config is in conf/app.toml:3."));
    let out = a.run("go").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);

    let reqs = fx.server.chat_requests();
    assert_eq!(reqs.len(), 6);
    // The ask comes last, and the tools stay so the cache holds.
    let ask = reqs[4]["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        ask,
        "Stopped: reached the limit of 3 model requests in one run. You can't call tools any more. Report what you found so far, with the paths and line numbers, and what is left undone."
    );
    assert_eq!(tool_names(&reqs[4]), tool_names(&reqs[3]));

    let (r, is_error) = &results(&a)[0];
    assert!(!is_error);
    assert_eq!(
        r,
        "The config is in conf/app.toml:3.\n\n[scout: 4 model requests, 3 tool calls; stopped: reached the limit of 3 model requests in one run; it wrote this report when stopped]"
    );
    let info = &subagent_info(&a)[0];
    assert_eq!(info["report"], true, "{info}");
    assert_eq!(info["stop"], "budget", "{info}");
}

#[tokio::test]
async fn a_last_report_that_only_calls_tools_runs_nothing() {
    let (fx, mut a) = stopped_scout(call("bash", json!({"command": "ls"})));
    a.run("go").await;
    // No request after the report's but the parent's.
    assert_eq!(fx.server.chat_requests().len(), 6);
    let (r, _) = &results(&a)[0];
    assert_eq!(
        r,
        "(no answer)\n\n[scout: 4 model requests, 3 tool calls; stopped: reached the limit of 3 model requests in one run]"
    );
    assert_eq!(subagent_info(&a)[0]["report"], false);
}
