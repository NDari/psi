#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! Esc at every stage of a run (unhappy paths, batch 2): while the answer
//! streams, while a tool runs, while summarizing,
//! during a retry's pause.
//! Each run must stop promptly with `Stop::Aborted`, leave a session that
//! opens, and the next run must work as if nothing happened.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use psi_agent::{Agent, AgentConfig, AgentEvent, Budgets, RunOutcome, Stop};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_session::{EntryBody, SessionFile, StopReason, text_of};
use psi_tools::{ToolCtx, core_tools};
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fx {
    _tmp: TempDir,
    cwd: PathBuf,
    home: PathBuf,
    server: ReplayServer,
}

impl Fx {
    fn new(replies: Vec<Reply>) -> Self {
        // Not under /tmp: kept next to the other test files.
        let t = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let tmp = tempfile::tempdir_in(t).unwrap();
        let cwd = tmp.path().join("project");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(home.join(".psi")).unwrap();
        Self {
            server: ReplayServer::start(replies).unwrap(),
            cwd,
            home,
            _tmp: tmp,
        }
    }

    fn agent(&self, n_ctx: u64, max_result: usize) -> Agent {
        let session = SessionFile::create(&self.home.join(".psi/sessions"), &self.cwd).unwrap();
        let cfg = AgentConfig {
            model: "local".into(),
            max_output: 512,
            n_ctx,
            enable_thinking: None,
            sampling: Default::default(),
            budgets: Budgets::default(),
            system_prompt: "sys".into(),
            tools: core_tools(),
            home: Some(self.home.clone()),
            attended: false,
            task_budgets: Budgets::default(),
        };
        let ctx = ToolCtx::new(&self.cwd, &self.home.join("tmp"), max_result);
        Agent::new(Client::new(&self.server.base_url()), session, cfg, ctx)
    }
}

/// Runs `prompt` and presses Esc when `when` sees the first matching event.
/// Returns the outcome, how long the run took after Esc, and the agent.
async fn esc_on(
    a: Agent,
    prompt: &str,
    when: impl Fn(&AgentEvent) -> bool + Send + 'static,
) -> (RunOutcome, Duration, Agent) {
    esc_after(a, prompt, when, || true).await
}

/// As [`esc_on`], pressing Esc only once `ready` also holds (polled).
async fn esc_after(
    a: Agent,
    prompt: &str,
    when: impl Fn(&AgentEvent) -> bool + Send + 'static,
    ready: impl Fn() -> bool + Send + 'static,
) -> (RunOutcome, Duration, Agent) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut a = a.with_events(tx);
    let handle = a.abort_handle();
    let pressed: Arc<std::sync::Mutex<Option<Instant>>> = Arc::default();
    let at = pressed.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if at.lock().unwrap().is_none() && when(&ev) {
                while !ready() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                *at.lock().unwrap() = Some(Instant::now());
                handle.lock().unwrap().cancel();
            }
        }
    });
    let out = tokio::time::timeout(Duration::from_secs(20), a.run(prompt))
        .await
        .expect("the run didn't stop");
    let after = pressed
        .lock()
        .unwrap()
        .expect("Esc was never pressed")
        .elapsed();
    (out, after, a)
}

/// The aborted run stopped promptly and its session opens.
fn stopped(out: &RunOutcome, after: Duration, a: &Agent) {
    assert_eq!(out.stop, Stop::Aborted, "{:?}", out.stop);
    assert!(after < Duration::from_secs(3), "took {after:?} after Esc");
    let s = SessionFile::open(a.session().path()).expect("the session opens");
    assert_eq!(s.entries().len(), a.session().entries().len());
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

/// A process with this id is gone (killed, not left running). ps works on
/// Linux and macOS; /proc is Linux only.
fn gone(pid: &str) -> bool {
    for _ in 0..50 {
        let ps = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid.trim()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&ps.stdout);
        if !ps.status.success() || stat.trim().is_empty() || stat.trim().starts_with('Z') {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[tokio::test]
async fn esc_while_the_answer_streams() {
    let fx = Fx::new(vec![Reply::hang("Thinking about"), text("hi")]);
    let a = fx.agent(32_768, 20_000);
    let (out, after, a) = esc_on(a, "go", |e| matches!(e, AgentEvent::TextDelta { .. })).await;
    stopped(&out, after, &a);
    let aborted = a
        .session()
        .entries()
        .iter()
        .rev()
        .find_map(|e| match &e.body {
            EntryBody::Message { stop_reason, .. } => *stop_reason,
            _ => None,
        });
    assert_eq!(aborted, Some(StopReason::Aborted));
    // The next run works and sends a well-formed history.
    let mut a = a;
    let out = a.run("again").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    assert_eq!(out.text, "hi");
    let reqs = fx.server.chat_requests();
    // The earlier prompt stays, merged with the new one (the roles must
    // alternate, and the aborted answer had nothing to send).
    let users: Vec<&str> = reqs[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "user")
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert_eq!(users, ["go\n\nagain"]);
}

#[tokio::test]
async fn esc_while_a_tool_runs_kills_it() {
    let fx = Fx::new(vec![
        call("bash", json!({"command": "echo $$ > pid; exec sleep 30"})),
        text("ok"),
    ]);
    let a = fx.agent(32_768, 20_000);
    let (out, after, mut a) = esc_on(a, "go", |e| matches!(e, AgentEvent::ToolStart { .. })).await;
    stopped(&out, after, &a);
    // Esc may land before bash wrote its pid; then nothing ran.
    if let Ok(pid) = std::fs::read_to_string(fx.cwd.join("pid")) {
        assert!(gone(&pid), "the command was left running");
    }
    assert!(results(&a)[0].1, "{:?}", results(&a));
    assert_eq!(a.run("again").await.stop, Stop::Done);
}

#[tokio::test]
async fn esc_while_summarizing_keeps_the_history() {
    let summary = "## Goal\nPort the parser.";
    let fx = Fx::new(vec![
        Reply::Chat(ScriptedReply {
            prompt_tokens: 9500,
            ..ScriptedReply::call("read", json!({"path": "f.txt"}))
        }),
        Reply::hang("## Goal"),
        // The next run summarizes again, then answers.
        text(summary),
        text("ok"),
    ]);
    std::fs::write(fx.cwd.join("f.txt"), "line\n".repeat(200)).unwrap();
    let mut a = fx.agent(16_384, 7372);
    for i in 0..30 {
        let id = format!("old{i}");
        let s = a.session_mut();
        s.append(EntryBody::Message {
            role: psi_session::Role::Assistant,
            content: vec![psi_session::Block::ToolCall {
                id: id.clone(),
                name: "read".into(),
                arguments: json!({"path": format!("old{i}.txt")}),
                raw: None,
            }],
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        })
        .unwrap();
        s.append(EntryBody::ToolResult {
            tool_call_id: id,
            tool_name: "read".into(),
            content: vec![psi_session::Block::Text {
                text: format!("old {i} ").repeat(80),
            }],
            is_error: false,
        })
        .unwrap();
    }
    // Esc once the summary request is waiting on the server.
    let requests = fx.server.chat_counter();
    let (out, after, mut a) = esc_after(
        a,
        "go on",
        |e| matches!(e, AgentEvent::Compacting { .. }),
        move || requests() >= 2,
    )
    .await;
    stopped(&out, after, &a);
    assert_eq!(fx.server.chat_requests().len(), 2);
    let compactions = |a: &Agent| {
        a.session()
            .entries()
            .iter()
            .filter(|e| matches!(e.body, EntryBody::Compaction { .. }))
            .count()
    };
    assert_eq!(compactions(&a), 0, "a cut summary isn't recorded");
    let out = a.run("continue").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    assert_eq!(compactions(&a), 1);
}

#[tokio::test]
async fn esc_during_a_retrys_pause() {
    let fx = Fx::new(vec![
        Reply::Error {
            status: 503,
            body: json!({"error": {"code": 503, "message": "Loading model"}}),
        },
        text("ok"),
    ]);
    let a = fx.agent(32_768, 20_000);
    let (out, after, mut a) = esc_on(a, "go", |e| matches!(e, AgentEvent::Retrying { .. })).await;
    stopped(&out, after, &a);
    assert_eq!(fx.server.chat_requests().len(), 1, "no retry after Esc");
    assert_eq!(a.run("again").await.stop, Stop::Done);
}
