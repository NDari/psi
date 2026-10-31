#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! The agent loop driven by the replay server (D-32): real client, real
//! tools, scripted or recorded model responses.

use std::path::{Path, PathBuf};
use std::time::Duration;

use psi_agent::{Agent, AgentConfig, AgentEvent, Budgets, RunOutcome, Stop};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_session::{EntryBody, SessionFile};
use psi_tools::{ToolCtx, core_tools};
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    _tmp: TempDir,
    cwd: PathBuf,
    sessions: PathBuf,
    server: ReplayServer,
}

impl Fixture {
    fn new(replies: Vec<Reply>) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("work/project");
        std::fs::create_dir_all(&cwd).unwrap();
        let sessions = tmp.path().join("sessions");
        Self {
            cwd,
            sessions,
            server: ReplayServer::start(replies).unwrap(),
            _tmp: tmp,
        }
    }

    fn agent(&self, budgets: Budgets) -> Agent {
        let session = SessionFile::create(&self.sessions, &self.cwd).unwrap();
        let ctx = ToolCtx::new(&self.cwd, &self.cwd.join("../tmp"), 20_000);
        let cfg = AgentConfig {
            model: "local".into(),
            max_output: 512,
            n_ctx: 8192,
            enable_thinking: None,
            budgets,
            system_prompt: "sys".into(),
            tools: core_tools(false),
            home: None,
        };
        Agent::new(Client::new(&self.server.base_url()), session, cfg, ctx)
    }

    async fn run(&self, input: &str) -> (RunOutcome, Agent) {
        let mut a = self.agent(Budgets::default());
        let out = a.run(input).await;
        (out, a)
    }
}

fn call(name: &str, args: Value) -> Reply {
    Reply::Chat(ScriptedReply::call(name, args))
}

fn text(t: &str) -> Reply {
    Reply::Chat(ScriptedReply::text(t))
}

fn tool_results(a: &Agent) -> Vec<(String, bool)> {
    a.session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::ToolResult {
                content, is_error, ..
            } => Some((psi_session::text_of(content), *is_error)),
            _ => None,
        })
        .collect()
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[tokio::test]
async fn write_then_answer() {
    let f = Fixture::new(vec![
        call("write", json!({"path": "hello.txt", "content": "hi\n"})),
        text("Wrote it."),
    ]);
    let (out, a) = f.run("make hello.txt").await;
    assert_eq!(
        out,
        RunOutcome {
            stop: Stop::Done,
            text: "Wrote it.".into(),
            turns: 2,
            tool_calls: 1
        }
    );
    assert_eq!(
        std::fs::read_to_string(f.cwd.join("hello.txt")).unwrap(),
        "hi\n"
    );
    // The second request carries the tool result, after an append-only prefix.
    let reqs = f.server.chat_requests();
    assert_eq!(reqs.len(), 2);
    let m = reqs[1]["messages"].as_array().unwrap();
    assert_eq!(m[0]["role"], "system");
    assert_eq!(m[1], json!({"role": "user", "content": "make hello.txt"}));
    assert_eq!(m[3]["role"], "tool");
    assert_eq!(m[3]["content"], "Created hello.txt (1 line).");
    assert_eq!(reqs[0]["messages"].as_array().unwrap()[..2], m[..2]);
    // Sampling is left to the server.
    assert!(reqs[0].get("temperature").is_none());
    assert_eq!(reqs[0]["tools"].as_array().unwrap().len(), 4);
    // The session reopens to the same branch.
    let again = SessionFile::open(a.session().path()).unwrap();
    assert_eq!(again.entries().len(), a.session().entries().len());
}

#[tokio::test]
async fn parallel_reads_keep_call_order() {
    let f = Fixture::new(vec![
        Reply::Chat(ScriptedReply {
            tool_calls: vec![
                ("read".into(), json!({"path": "a"}).to_string()),
                ("read".into(), json!({"path": "b"}).to_string()),
            ],
            ..Default::default()
        }),
        text("ok"),
    ]);
    std::fs::write(f.cwd.join("a"), "A\n").unwrap();
    std::fs::write(f.cwd.join("b"), "B\n").unwrap();
    let (_, a) = f.run("read both").await;
    assert_eq!(
        tool_results(&a),
        [("1\tA".into(), false), ("1\tB".into(), false)]
    );
}

#[tokio::test]
async fn recorded_cut_off_tool_call_is_not_run() {
    // Qwen3.5 4B's real stream, cut off by max_tokens inside write's arguments.
    let raw = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../psi-ai/tests/fixtures/llama-server/qwen3.5-4b/length_tool_call.response.http");
    let f = Fixture::new(vec![Reply::from_http_file(&raw).unwrap(), text("ok")]);
    let (out, a) = f.run("write a poem").await;
    assert_eq!(out.stop, Stop::Done);
    let results = tool_results(&a);
    assert_eq!(results.len(), 1);
    assert!(results[0].1);
    assert!(
        results[0]
            .0
            .starts_with("This call was cut off by the output token limit"),
        "{}",
        results[0].0
    );
    assert!(
        std::fs::read_dir(&f.cwd).unwrap().next().is_none(),
        "nothing was written"
    );
    // The cut-off arguments are not sent back: llama-server would reject the
    // request (Gemma 4, psi eval sh_logreport, 2026-10-31).
    let m = f.server.chat_requests()[1]["messages"].clone();
    assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], "{}");
}

#[tokio::test]
async fn gemma_edit_shapes_from_real_transcripts() {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir().join("gemma-edit-shapes.json")).unwrap(),
    )
    .unwrap();
    for case in cases {
        let task = case["task"].as_str().unwrap();
        let f = Fixture::new(vec![call("edit", case["arguments"].clone()), text("done")]);
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../evals/code")
            .join(task)
            .join("repo");
        copy_dir(&repo, &f.cwd);
        let (_, a) = f.run("fix it").await;
        let results = tool_results(&a);
        // pi refused both; psi takes the one replacement in edits[] and the
        // path inside it, and drops the pair that changed nothing.
        assert!(!results[0].1, "{task}: {}", results[0].0);
        assert!(
            results[0].0.starts_with("Edited "),
            "{task}: {}",
            results[0].0
        );
        let notes: Vec<&EntryBody> = a.session().entries().iter().map(|e| &e.body).filter(|b| matches!(b, EntryBody::Custom { custom_type, .. } if custom_type == "shape_fix")).collect();
        assert_eq!(notes.len(), 1, "{task}");
    }
}

#[tokio::test]
async fn shape_refusal_goes_back_to_the_model() {
    let f = Fixture::new(vec![
        call("edit", json!({"old_text": "a", "new_text": "b"})),
        text("sorry"),
    ]);
    let (_, a) = f.run("x").await;
    assert_eq!(
        tool_results(&a)[0].0,
        r#"edit is missing 'path'. Example: edit {"path":"app.py","old_text":"    return a - b","new_text":"    return a + b"}"#
    );
}

#[tokio::test]
async fn gemma_write_without_path_gets_a_path_first_example() {
    // The shape of Gemma 4's 20 failed writes (psi eval, sh_logreport):
    // content (now an alias of text) and intent, no path.
    let f = Fixture::new(vec![
        call(
            "write",
            json!({"content": "#!/bin/bash\necho hi\n", "intent": "Create the script."}),
        ),
        call(
            "write",
            json!({"intent": "Create the script.", "path": "report.sh", "content": "#!/bin/bash\necho hi\n"}),
        ),
        text("done"),
    ]);
    let (_, a) = f.run("write report.sh").await;
    let r = tool_results(&a);
    assert_eq!(
        r[0].0,
        r#"write is missing 'path'. Example: write {"path":"notes/todo.md","text":"- first item\n"}"#
    );
    assert_eq!(r[1], ("Created report.sh (2 lines).".into(), false));
}

#[tokio::test]
async fn write_after_a_bash_rewrite_reads_for_the_model() {
    // Gemma 4 (psi eval, sh_logreport): write, then rewrite the file with a
    // bash heredoc, then write again. The refusal shows the current content
    // and counts as the read, so the retry goes through instead of looping.
    let f = Fixture::new(vec![
        call("write", json!({"path": "r.sh", "text": "v1\n"})),
        call("bash", json!({"command": "printf 'v2\\n' > r.sh"})),
        call("write", json!({"path": "r.sh", "text": "v3\n"})),
        call("write", json!({"path": "r.sh", "text": "v3\n"})),
        text("done"),
    ]);
    let (_, a) = f.run("x").await;
    let r = tool_results(&a);
    assert!(r[2].1);
    assert!(
        r[2].0
            .starts_with("r.sh changed since you last read it, so it was not replaced."),
        "{}",
        r[2].0
    );
    assert!(r[2].0.ends_with("\n1\tv2"), "{}", r[2].0);
    assert_eq!(r[3], ("Replaced r.sh (1 line).".into(), false));
    assert_eq!(std::fs::read_to_string(f.cwd.join("r.sh")).unwrap(), "v3\n");
}

#[tokio::test]
async fn turn_budget_stops_the_run() {
    let replies = (0..5)
        .map(|_| call("bash", json!({"command": "true"})))
        .collect();
    let f = Fixture::new(replies);
    let mut a = f.agent(Budgets {
        max_turns: 3,
        ..Budgets::default()
    });
    let out = a.run("loop").await;
    assert_eq!(
        out.stop,
        Stop::Budget("reached the limit of 3 model requests in one run".into())
    );
    assert_eq!(out.turns, 3);
}

#[tokio::test]
async fn consecutive_errors_stop_the_run() {
    let replies = (0..5)
        .map(|_| call("read", json!({"path": "missing"})))
        .collect();
    let f = Fixture::new(replies);
    let mut a = f.agent(Budgets {
        max_consecutive_errors: 2,
        ..Budgets::default()
    });
    assert_eq!(
        a.run("x").await.stop,
        Stop::Budget("2 tool calls in a row failed".into())
    );
}

#[tokio::test]
async fn template_error_fails_without_retry() {
    let raw = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../psi-ai/tests/fixtures/llama-server/qwen3.5-4b/system_not_first.response.http");
    let f = Fixture::new(vec![Reply::from_http_file(&raw).unwrap(), text("never")]);
    let (out, _) = f.run("x").await;
    assert!(
        matches!(&out.stop, Stop::Failed(m) if m.contains("chat template")),
        "{:?}",
        out.stop
    );
    assert_eq!(f.server.chat_requests().len(), 1);
}

#[tokio::test]
async fn context_overflow_drops_old_turns_and_retries() {
    let raw = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../psi-ai/tests/fixtures/llama-server/qwen3.5-4b/overflow.response.http");
    let mut replies: Vec<Reply> = (0..4)
        .map(|i| call("bash", json!({"command": format!("seq 1 {}", 2000 + i)})))
        .collect();
    replies.push(Reply::from_http_file(&raw).unwrap());
    replies.push(text("done"));
    let f = Fixture::new(replies);
    let (out, a) = f.run("count").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    let reqs = f.server.chat_requests();
    let last = reqs.last().unwrap()["messages"].as_array().unwrap().clone();
    assert!(
        last[1]["content"]
            .as_str()
            .unwrap()
            .ends_with(psi_agent_note())
    );
    assert!(last.len() < reqs[4]["messages"].as_array().unwrap().len());
    assert!(a.session().entries().iter().any(
        |e| matches!(&e.body, EntryBody::GuardEvent { guard, .. } if guard == "context_overflow")
    ));
}

fn psi_agent_note() -> &'static str {
    "(Earlier turns of this conversation were removed to fit the context window.)"
}

#[tokio::test]
async fn abort_stops_a_running_tool() {
    let f = Fixture::new(vec![
        call("bash", json!({"command": "sleep 30"})),
        text("never"),
    ]);
    let mut a = f.agent(Budgets::default());
    let cancel = a.cancel_token();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel();
    });
    let out = a.run("x").await;
    assert_eq!(out.stop, Stop::Aborted);
    assert!(tool_results(&a)[0].0.ends_with("[aborted]"));
}

#[tokio::test]
async fn steering_and_follow_up() {
    let f = Fixture::new(vec![
        call("bash", json!({"command": "true"})),
        text("first"),
        text("second"),
    ]);
    let mut a = f.agent(Budgets::default());
    let q = a.queues();
    q.steering.lock().unwrap().push_back("use python".into());
    q.follow_up.lock().unwrap().push_back("now explain".into());
    let out = a.run("start").await;
    assert_eq!(out.text, "second");
    let reqs = f.server.chat_requests();
    let m1 = reqs[1]["messages"].as_array().unwrap();
    assert_eq!(
        m1.last().unwrap(),
        &json!({"role": "user", "content": "use python"})
    );
    let m2 = reqs[2]["messages"].as_array().unwrap();
    assert_eq!(
        m2.last().unwrap(),
        &json!({"role": "user", "content": "now explain"})
    );
}

#[tokio::test]
async fn events_stream_in_order() {
    let f = Fixture::new(vec![
        call("bash", json!({"command": "echo hi"})),
        text("bye"),
    ]);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut a = f.agent(Budgets::default()).with_events(tx);
    a.run("x").await;
    drop(a);
    let mut kinds = Vec::new();
    while let Some(ev) = rx.recv().await {
        kinds.push(
            serde_json::to_value(&ev).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
        if matches!(ev, AgentEvent::RunEnd { .. }) {
            break;
        }
    }
    assert_eq!(kinds.first().map(String::as_str), Some("run_start"));
    assert_eq!(kinds.last().map(String::as_str), Some("run_end"));
    for k in [
        "turn_start",
        "tool_start",
        "tool_end",
        "text_delta",
        "entry",
        "turn_end",
    ] {
        assert!(kinds.iter().any(|x| x == k), "{k} missing from {kinds:?}");
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}
