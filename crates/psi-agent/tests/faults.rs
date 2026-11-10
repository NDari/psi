#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! Unhappy paths: the model server failing in every way it can, through the
//! real client and loop. Each case checks three things: the run ends (no
//! hang) with a reason that says what happened, it retries only what may
//! pass, and the session still opens afterwards.

use std::path::PathBuf;
use std::time::Duration;

use psi_agent::{Agent, AgentConfig, AgentEvent, Budgets, RunOutcome, Stop};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_session::{EntryBody, SessionFile};
use psi_tools::{ToolCtx, core_tools};
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fx {
    _tmp: TempDir,
    cwd: PathBuf,
    sessions: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        Self {
            sessions: tmp.path().join("sessions"),
            cwd,
            _tmp: tmp,
        }
    }

    fn agent(&self, url: &str) -> Agent {
        let session = SessionFile::create(&self.sessions, &self.cwd).unwrap();
        let cfg = AgentConfig {
            model: "local".into(),
            max_output: 512,
            n_ctx: 8192,
            enable_thinking: None,
            budgets: Budgets::default(),
            system_prompt: "sys".into(),
            tools: core_tools(),
            home: None,
            attended: false,
            task_budgets: Budgets::default(),
        };
        let ctx = ToolCtx::new(&self.cwd, &self.cwd.join("../tmp"), 20_000);
        Agent::new(Client::new(url), session, cfg, ctx)
    }
}

/// Runs `prompt` with a time limit, collecting its notices and retries.
async fn run(a: Agent, prompt: &str) -> (RunOutcome, Agent, Vec<String>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut a = a.with_events(tx);
    let out = tokio::time::timeout(Duration::from_secs(20), a.run(prompt))
        .await
        .expect("the run hung");
    let mut notices = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        match ev {
            AgentEvent::Notice { text } => notices.push(text),
            AgentEvent::Retrying { reason } => notices.push(format!("{reason}; trying again")),
            _ => {}
        }
    }
    (out, a, notices)
}

/// The session file opens and holds what the agent has.
fn reopens(a: &Agent) {
    let s = SessionFile::open(a.session().path()).expect("the session opens");
    assert_eq!(s.entries().len(), a.session().entries().len());
}

/// The error recorded on the last assistant entry.
fn last_error(a: &Agent) -> Option<String> {
    a.session()
        .entries()
        .iter()
        .rev()
        .find_map(|e| match &e.body {
            EntryBody::Message { error, .. } => error.clone(),
            _ => None,
        })
}

fn text(t: &str) -> Reply {
    Reply::Chat(ScriptedReply::text(t))
}

fn error(status: u16, message: &str) -> Reply {
    Reply::Error {
        status,
        body: json!({"error": {"code": status, "message": message, "type": "server_error"}}),
    }
}

fn failed(out: &RunOutcome) -> &str {
    match &out.stop {
        Stop::Failed(r) => r,
        s => panic!("expected a failure, got {s:?}"),
    }
}

#[tokio::test]
async fn a_loading_server_is_retried_once() {
    let fx = Fx::new();
    let s = ReplayServer::start(vec![error(503, "Loading model"), text("ok")]).unwrap();
    let (out, a, notices) = run(fx.agent(&s.base_url()), "hi").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    assert_eq!(out.text, "ok");
    assert_eq!(s.chat_requests().len(), 2);
    assert_eq!(last_error(&a), None);
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].contains("503") && notices[0].ends_with("trying again"),
        "{notices:?}"
    );
    reopens(&a);
}

#[tokio::test]
async fn a_server_that_stays_down_fails_with_its_message() {
    let fx = Fx::new();
    let s = ReplayServer::start(vec![
        error(503, "Loading model"),
        error(503, "Loading model"),
        text("never"),
    ])
    .unwrap();
    let (out, a, _) = run(fx.agent(&s.base_url()), "hi").await;
    let why = failed(&out);
    assert!(
        why.contains("503") && why.contains("Loading model"),
        "{why}"
    );
    assert_eq!(s.chat_requests().len(), 2);
    assert_eq!(s.remaining(), 1);
    assert_eq!(last_error(&a).as_deref(), Some(why));
    reopens(&a);
}

#[tokio::test]
async fn other_server_errors_are_not_retried() {
    let fx = Fx::new();
    let s = ReplayServer::start(vec![error(500, "CUDA error: out of memory"), text("x")]).unwrap();
    let (out, a, notices) = run(fx.agent(&s.base_url()), "hi").await;
    assert!(failed(&out).contains("out of memory"));
    assert_eq!(s.chat_requests().len(), 1);
    assert!(notices.is_empty());
    reopens(&a);
}

#[tokio::test]
async fn a_cut_answer_is_retried_not_taken_as_done() {
    let fx = Fx::new();
    let s =
        ReplayServer::start(vec![Reply::cut("The answer is"), text("The answer is 42.")]).unwrap();
    let (out, a, notices) = run(fx.agent(&s.base_url()), "hi").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    assert_eq!(out.text, "The answer is 42.");
    assert!(
        notices[0].contains("closed the stream before the answer finished"),
        "{notices:?}"
    );
    reopens(&a);
    // Cut twice: a failure that says so, not a half answer.
    let s = ReplayServer::start(vec![Reply::cut("The"), Reply::cut("The")]).unwrap();
    let (out, a, _) = run(fx.agent(&s.base_url()), "hi").await;
    assert!(failed(&out).contains("closed the stream before the answer finished"));
    reopens(&a);
}

#[tokio::test]
async fn errors_and_garbage_inside_the_stream() {
    let fx = Fx::new();
    let s = ReplayServer::start(vec![Reply::stream_error("slot unavailable"), text("ok")]).unwrap();
    let (out, _, notices) = run(fx.agent(&s.base_url()), "hi").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    assert!(notices[0].contains("slot unavailable"), "{notices:?}");
    let s = ReplayServer::start(vec![Reply::garbage(), Reply::garbage()]).unwrap();
    let (out, a, _) = run(fx.agent(&s.base_url()), "hi").await;
    assert!(failed(&out).contains("Bad Gateway"), "{:?}", out.stop);
    reopens(&a);
}

#[tokio::test]
async fn an_unreachable_server_names_its_address() {
    let fx = Fx::new();
    // Port 1: nothing listens there, and the OS never hands it to another
    // test's server (a port freed here could go to one running in parallel).
    let port = 1;
    let url = format!("http://127.0.0.1:{port}");
    let (out, a, notices) = run(fx.agent(&url), "hi").await;
    let why = failed(&out);
    assert!(
        why.starts_with(&format!("cannot reach the model server at {url}")),
        "{why}"
    );
    assert_eq!(notices.len(), 1, "one retry");
    reopens(&a);
}

#[tokio::test]
async fn context_overflow_gives_up_after_three_tries() {
    let fx = Fx::new();
    let overflow = || Reply::Error {
        status: 400,
        body: json!({"error": {"code": 400, "type": "exceed_context_size_error",
            "message": "the request exceeds the available context size",
            "n_prompt_tokens": 9000, "n_ctx": 8192}}),
    };
    let s = ReplayServer::start((0..5).map(|_| overflow()).collect()).unwrap();
    let mut a = fx.agent(&s.base_url());
    // Earlier turns to drop.
    for i in 0..6 {
        a.session_mut()
            .append(EntryBody::Message {
                role: psi_session::Role::User,
                content: vec![psi_session::Block::Text {
                    text: format!("old {i} ").repeat(50),
                }],
                usage: None,
                stop_reason: None,
                error: None,
                model: None,
            })
            .unwrap();
    }
    let (out, a, _) = run(a, "hi").await;
    let why = failed(&out);
    assert!(why.contains("does not fit the context window"), "{why}");
    assert!(s.chat_requests().len() <= 4, "{}", s.chat_requests().len());
    reopens(&a);
}

/// The requests the server saw carry no trace of a failed attempt.
#[tokio::test]
async fn a_retry_sends_the_same_request() {
    let fx = Fx::new();
    let s = ReplayServer::start(vec![Reply::cut("x"), text("ok")]).unwrap();
    run(fx.agent(&s.base_url()), "hi").await;
    let reqs: Vec<Value> = s.chat_requests();
    assert_eq!(reqs[0]["messages"], reqs[1]["messages"]);
}
