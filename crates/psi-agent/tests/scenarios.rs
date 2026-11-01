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
        self.agent_with(budgets, core_tools(false))
    }

    fn agent_with(&self, budgets: Budgets, tools: Vec<psi_tools::Tool>) -> Agent {
        self.agent_sized(budgets, tools, 8192, 20_000)
    }

    /// An agent with its own window and tool result cap (characters).
    fn agent_sized(
        &self,
        budgets: Budgets,
        tools: Vec<psi_tools::Tool>,
        n_ctx: u64,
        max_result_chars: usize,
    ) -> Agent {
        let session = SessionFile::create(&self.sessions, &self.cwd).unwrap();
        let ctx = ToolCtx::new(&self.cwd, &self.cwd.join("../tmp"), max_result_chars);
        let cfg = AgentConfig {
            model: "local".into(),
            max_output: 512,
            n_ctx,
            enable_thinking: None,
            budgets,
            system_prompt: "sys".into(),
            tools,
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
async fn error_streaks_nudge_then_stop_headless_runs() {
    let replies = (0..6)
        .map(|i| call("read", json!({"path": format!("missing{i}")})))
        .collect();
    let f = Fixture::new(replies);
    let mut a = f.agent(Budgets {
        max_consecutive_errors: 2,
        ..Budgets::default()
    });
    // A nudge after 2 failures in a row; after 4 the run pauses, and a
    // headless run stops.
    assert_eq!(
        a.run("x").await.stop,
        Stop::Guard("4 tool calls in a row failed".into())
    );
    let results = tool_results(&a);
    assert_eq!(results.len(), 4);
    assert!(
        results[1].0.ends_with(
            "Re-read the relevant files and the last error, and re-plan before the next call."
        ),
        "{results:?}"
    );
    let guards: Vec<String> = a
        .session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::GuardEvent { guard, .. } => Some(guard.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(guards, ["error_streak", "error_streak"]);
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
    // A window the harness thinks is large: only the server's overflow cuts.
    let mut a = f.agent_sized(Budgets::default(), core_tools(false), 1 << 20, 20_000);
    let out = a.run("count").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    let reqs = f.server.chat_requests();
    let last = reqs.last().unwrap()["messages"].as_array().unwrap().clone();
    assert!(
        last[1]["content"]
            .as_str()
            .unwrap()
            .starts_with(psi_agent_note())
    );
    assert!(last.len() < reqs[4]["messages"].as_array().unwrap().len());
    assert!(a.session().entries().iter().any(
        |e| matches!(&e.body, EntryBody::GuardEvent { guard, .. } if guard == "context_overflow")
    ));
}

/// Text files of `lines` numbered lines each, named f0.txt, f1.txt, ...
fn write_files(cwd: &Path, n: usize, lines: usize) {
    for i in 0..n {
        let body: String = (0..lines)
            .map(|l| format!("file {i} line {l:03}\n"))
            .collect();
        std::fs::write(cwd.join(format!("f{i}.txt")), body).unwrap();
    }
}

fn reads(n: usize) -> Vec<Reply> {
    (0..n)
        .map(|i| call("read", json!({"path": format!("f{i}.txt")})))
        .collect()
}

/// The kinds of entries a run wrote, for following what happened.
fn kinds(a: &Agent) -> Vec<String> {
    a.session()
        .entries()
        .iter()
        .map(|e| match &e.body {
            EntryBody::ToolResult { .. } => "result".into(),
            EntryBody::ClearedResults { entry_ids, .. } => format!("cleared {}", entry_ids.len()),
            EntryBody::Compaction { replaces, .. } if !replaces.is_empty() => "merge".into(),
            EntryBody::Compaction { .. } => "compaction".into(),
            EntryBody::GuardEvent { guard, .. } => format!("guard {guard}"),
            _ => String::new(),
        })
        .filter(|k| !k.is_empty())
        .collect()
}

#[tokio::test]
async fn old_tool_results_are_cleared_past_the_trigger() {
    let mut replies = reads(8);
    replies.push(text("done"));
    let f = Fixture::new(replies);
    write_files(&f.cwd, 8, 200);
    let mut a = f.agent_sized(Budgets::default(), core_tools(false), 16_384, 7372);
    let out = a.run("read the files").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    // One batch, once past the trigger; no summary was needed.
    assert_eq!(kinds(&a)[7], "cleared 3", "{:?}", kinds(&a));
    assert!(!kinds(&a).contains(&"compaction".to_owned()));
    let freed: Vec<u64> = a
        .session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::ClearedResults { tokens_freed, .. } => Some(*tokens_freed),
            _ => None,
        })
        .collect();
    assert!(freed.iter().all(|&t| t >= 16_384 / 10), "{freed:?}");
    let reqs = f.server.chat_requests();
    let last = reqs.last().unwrap()["messages"].as_array().unwrap();
    let results: Vec<&str> = last
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert_eq!(results.len(), 8);
    assert_eq!(
        results[0],
        "(Result cleared to save context: read f0.txt, 200 lines. Run it again if you need it.)"
    );
    // The most recent are sent whole, and the session keeps them all.
    for r in &results[5..] {
        assert!(r.contains("line 199"), "{r}");
    }
    assert!(tool_results(&a).iter().all(|(t, _)| t.contains("line 199")));
}

/// A session with a task holding planted facts, then `turns` small read
/// turns (too small to clear), as if from earlier work.
fn prefill(a: &mut Agent, turns: usize) {
    let s = a.session_mut();
    s.append(user_entry(
        "Port the parser. The code word is PELICAN-42; keep the limit at 7 retries.",
    ))
    .unwrap();
    for i in 0..turns {
        let id = format!("old{i}");
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
}

fn user_entry(t: &str) -> EntryBody {
    EntryBody::Message {
        role: psi_session::Role::User,
        content: vec![psi_session::Block::Text { text: t.into() }],
        usage: None,
        stop_reason: None,
        error: None,
        model: None,
    }
}

/// A read call whose response says the prompt was `prompt_tokens` long.
fn read_at(path: &str, prompt_tokens: u64) -> Reply {
    Reply::Chat(ScriptedReply {
        prompt_tokens,
        ..ScriptedReply::call("read", json!({"path": path}))
    })
}

fn compaction_info(a: &Agent) -> Vec<Value> {
    a.session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::Custom { custom_type, data } if custom_type == "compaction" => {
                Some(data.clone())
            }
            _ => None,
        })
        .collect()
}

const SUMMARY: &str =
    "## Goal\nPort the parser.\n## Constraints and facts\nCode word PELICAN-42; 7 retries.";

#[tokio::test]
async fn a_summary_extends_the_last_request_and_keeps_a_tail() {
    let f = Fixture::new(vec![read_at("f0.txt", 9500), text(SUMMARY), text("done")]);
    write_files(&f.cwd, 1, 200);
    let mut a = f.agent_sized(Budgets::default(), core_tools(false), 16_384, 7372);
    prefill(&mut a, 30);
    let out = a.run("go on").await;
    assert_eq!(out.stop, Stop::Done, "{:?} {:?}", out.stop, kinds(&a));
    assert_eq!(compaction_info(&a)[0]["method"], "extend");
    let reqs = f.server.chat_requests();
    assert_eq!(reqs.len(), 3);
    // The summary request is the last request plus the new result and
    // the instruction, with the same tools (D-38).
    let first = reqs[0]["messages"].as_array().unwrap();
    let sum = reqs[1]["messages"].as_array().unwrap();
    assert_eq!(sum[..first.len()], first[..]);
    assert_eq!(reqs[1]["tools"], reqs[0]["tools"]);
    let instruction = sum.last().unwrap()["content"].as_str().unwrap();
    assert!(instruction.contains("Summarize the conversation so far"));
    // After it: the summary, the task repeated (the cut is inside it), and
    // the kept tail as it was.
    let next = reqs[2]["messages"].as_array().unwrap();
    let opening = next[1]["content"].as_str().unwrap();
    assert!(opening.starts_with("[Summary of earlier work"), "{opening}");
    assert!(opening.contains(SUMMARY));
    assert!(opening.contains("[The last user message, repeated]\n\nPort the parser."));
    assert!(next.len() < first.len());
    let tail = &next[2..];
    assert_eq!(tail[..], sum[sum.len() - 1 - tail.len()..sum.len() - 1]);
    // The session keeps everything.
    assert_eq!(tool_results(&a).len(), 31);
}

#[tokio::test]
async fn a_summary_falls_back_to_a_plain_request() {
    let f = Fixture::new(vec![
        read_at("f0.txt", 9500),
        call("read", json!({"path": "x"})),
        text(SUMMARY),
        text("done"),
    ]);
    write_files(&f.cwd, 1, 200);
    let mut a = f.agent_sized(Budgets::default(), core_tools(false), 16_384, 7372);
    prefill(&mut a, 30);
    let out = a.run("go on").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    let info = &compaction_info(&a)[0];
    assert_eq!(info["method"], "plain");
    assert_eq!(info["fallback_reason"], "the model called a tool");
    let reqs = f.server.chat_requests();
    let plain = &reqs[2];
    assert!(
        plain
            .get("tools")
            .is_none_or(|t| t.as_array().is_some_and(Vec::is_empty))
    );
    let m = plain["messages"].as_array().unwrap();
    assert_eq!(m.len(), 2);
    let body = m[1]["content"].as_str().unwrap();
    assert!(
        body.starts_with("<conversation>\nUser: Port the parser."),
        "{body}"
    );
    assert!(body.contains("Result of read: old 0"));
    // The plain request covers only what the summary replaces.
    assert!(!body.contains("go on"));
    assert!(
        reqs[3]["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains(SUMMARY)
    );
}

#[tokio::test]
async fn summaries_chain_and_the_oldest_merge() {
    let s1 = format!("S1 PELICAN-42 {}", "one ".repeat(700));
    let s2 = format!("S2 {}", "two ".repeat(700));
    let merged = "M PELICAN-42";
    let f = Fixture::new(vec![
        read_at("f0.txt", 9500),
        text(&s1),
        read_at("f1.txt", 9500),
        text(&s2),
        text(merged),
        text("done"),
    ]);
    write_files(&f.cwd, 2, 200);
    let mut a = f.agent_sized(Budgets::default(), core_tools(false), 16_384, 7372);
    prefill(&mut a, 30);
    let out = a.run("go on").await;
    assert_eq!(out.stop, Stop::Done, "{:?} {:?}", out.stop, kinds(&a));
    let methods: Vec<Value> = compaction_info(&a)
        .iter()
        .map(|i| i["method"].clone())
        .collect();
    assert_eq!(methods, ["extend", "extend", "merge"]);
    let reqs = f.server.chat_requests();
    // The second summary request carries the first summary and asks only
    // for what came after it.
    let second = reqs[3]["messages"].as_array().unwrap();
    assert!(
        second[1]["content"]
            .as_str()
            .unwrap()
            .contains("S1 PELICAN-42")
    );
    assert!(
        second.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("everything after the last summary")
    );
    // The merge request has both, oldest first; afterwards one block.
    let merge = reqs[4]["messages"][1]["content"].as_str().unwrap();
    assert!(merge.find("S1 PELICAN").unwrap() < merge.find("S2 two").unwrap());
    let last = reqs[5]["messages"][1]["content"].as_str().unwrap();
    assert!(last.contains(merged));
    assert!(!last.contains("S1 PELICAN") && !last.contains("S2 two"));
    assert_eq!(last.matches("[Summary of earlier work").count(), 1);
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

/// Gemma 4 12B ended runs by announcing a step without making the call
/// (psi eval fix_invoice_rounding and multi_sqlite_migration, 2026-10-31).
/// A headless run gets one nudge and goes on.
#[tokio::test]
async fn an_announced_step_gets_one_nudge() {
    let f = Fixture::new(vec![
        text("The totals are off by a cent.\n\nNow I will examine `billing/money.py`."),
        call("read", json!({"path": "money.py"})),
        text("Now I will fix it."),
    ]);
    std::fs::write(f.cwd.join("money.py"), "x = 1\n").unwrap();
    let (out, a) = f.run("fix the rounding").await;
    assert_eq!(out.stop, Stop::Done);
    assert_eq!(out.turns, 3, "the second announcement ends the run");
    let reqs = f.server.chat_requests();
    let m = reqs[1]["messages"].as_array().unwrap();
    assert_eq!(m.last().unwrap()["role"], "user");
    assert!(
        m.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .starts_with("You described a next step but made no tool call")
    );
    assert!(a.session().entries().iter().any(
        |e| matches!(&e.body, EntryBody::GuardEvent { guard, .. } if guard == "announced_stop")
    ));
}

/// Gemma 4 12B rambled until max_tokens with no tool call (psi eval
/// multi_split_package, 2026-10-31), and the run ended as if it had
/// answered. A cut-off answer gets another turn, at most twice per run.
#[tokio::test]
async fn an_answer_cut_off_by_the_output_limit_continues() {
    let cut = || {
        Reply::Chat(ScriptedReply {
            text: "I'll rewrite it.\n\nWait, I'll just".into(),
            finish: Some("length".into()),
            ..Default::default()
        })
    };
    let f = Fixture::new(vec![cut(), cut(), cut(), text("never")]);
    let (out, a) = f.run("split the module").await;
    assert_eq!(out.stop, Stop::Done);
    assert_eq!(out.turns, 3, "two retries, then the run ends");
    let m = f.server.chat_requests()[1]["messages"].clone();
    let last = m.as_array().unwrap().last().unwrap().clone();
    assert!(
        last["content"]
            .as_str()
            .unwrap()
            .starts_with("Your reply hit the output limit (512 tokens)"),
        "{last}"
    );
    let n = a.session().entries().iter().filter(|e| matches!(&e.body, EntryBody::GuardEvent { guard, .. } if guard == "truncated_answer")).count();
    assert_eq!(n, 2);
}

/// D-36: the check runs once per turn with edits, after the last one that
/// worked; turns without edits don't run it.
#[tokio::test]
async fn post_edit_check_once_per_turn_with_edits() {
    let f = Fixture::new(vec![
        Reply::Chat(ScriptedReply {
            tool_calls: vec![("read".into(), json!({"path": "a"}).to_string())],
            ..Default::default()
        }),
        Reply::Chat(ScriptedReply {
            tool_calls: vec![
                (
                    "write".into(),
                    json!({"path": "b", "text": "B\n"}).to_string(),
                ),
                (
                    "write".into(),
                    json!({"path": "c", "text": "C\n"}).to_string(),
                ),
                ("bash".into(), json!({"command": "true"}).to_string()),
            ],
            ..Default::default()
        }),
        call("write", json!({"path": "bad", "text": "x\n"})),
        text("done"),
    ]);
    std::fs::write(f.cwd.join("a"), "A\n").unwrap();
    // Counts its runs, and fails once the file "bad" exists.
    let check = psi_agent::PostEditCheck {
        command: "echo run >> ../checks; test ! -e bad || { echo 'bad: not allowed'; exit 1; }"
            .into(),
        timeout_s: 10,
    };
    let mut a = f
        .agent(Budgets::default())
        .with_post_edit_check(Some(check));
    let out = a.run("go").await;
    assert_eq!(out.stop, Stop::Done);
    let r = tool_results(&a);
    assert_eq!(r[0], ("1\tA".into(), false));
    assert!(!r[1].0.contains("post-edit"), "{}", r[1].0);
    assert!(
        r[2].0.ends_with(
            "\n\n[post-edit check passed: `echo run >> ../checks; test ! -e bad || { echo 'bad: not ...`]"
        ),
        "{}",
        r[2].0
    );
    assert!(!r[3].0.contains("post-edit"));
    // A failed check doesn't make the edit a failed call.
    assert!(!r[4].1);
    assert!(
        r[4].0.contains("\n\n[post-edit check failed: `")
            && r[4].0.ends_with("`]\nbad: not allowed\n[exit code 1]"),
        "{}",
        r[4].0
    );
    let runs = std::fs::read_to_string(f.cwd.join("../checks")).unwrap();
    assert_eq!(runs.lines().count(), 2);
    let recorded = a
        .session()
        .entries()
        .iter()
        .filter(|e| matches!(&e.body, EntryBody::Custom { custom_type, .. } if custom_type == "post_edit_check"))
        .count();
    assert_eq!(recorded, 2);
}

/// D-34: the todo list lives in the session and follows the branch.
#[tokio::test]
async fn todo_list_follows_the_branch() {
    let f = Fixture::new(vec![
        Reply::Chat(ScriptedReply {
            tool_calls: vec![
                (
                    "todo".into(),
                    json!({"action": "add", "text": "a"}).to_string(),
                ),
                (
                    "todos".into(),
                    json!({"action": "add", "text": "b"}).to_string(),
                ),
                (
                    "todo".into(),
                    json!({"action": "toggle", "id": "1"}).to_string(),
                ),
            ],
            ..Default::default()
        }),
        text("planned"),
        call("todo", json!({"action": "list"})),
        text("listed"),
        call("todo", json!({"action": "list"})),
        text("empty"),
    ]);
    let mut tools = core_tools(false);
    tools.push(psi_tools::Tool::Todo);
    let mut a = f.agent_with(Budgets::default(), tools);
    a.run("plan").await;
    let first_prompt = a
        .session()
        .entries()
        .iter()
        .find(|e| {
            matches!(
                &e.body,
                EntryBody::Message {
                    role: psi_session::Role::User,
                    ..
                }
            )
        })
        .and_then(|e| e.parent_id.clone())
        .unwrap();
    a.run("list them").await;
    let r = tool_results(&a);
    assert_eq!(r[0].0, "Added #1: a");
    assert_eq!(r[1].0, "Added #2: b", "the alias resolves to todo");
    assert_eq!(r[2].0, "#1 is done: a");
    assert_eq!(r[3], ("[x] #1: a\n[ ] #2: b".into(), false));
    // The todo calls didn't make checkpoints or ask.
    assert!(
        !a.session().entries().iter().any(
            |e| matches!(&e.body, EntryBody::Checkpoint { trigger: Some(t), .. } if t == "tool")
        )
    );
    // Back before the first prompt: no todos on that branch.
    a.session_mut().set_leaf(&first_prompt).unwrap();
    a.run("again").await;
    assert_eq!(tool_results(&a).last().unwrap().0, "No todos.");
}

/// D-34: plan mode offers no write or edit, runs only read-only bash, adds
/// its context, and takes the plan from the answer; carrying it out tracks
/// [DONE:n] markers.
#[tokio::test]
async fn plan_mode_then_carry_out_the_plan() {
    let f = Fixture::new(vec![
        Reply::Chat(ScriptedReply {
            tool_calls: vec![
                (
                    "write".into(),
                    json!({"path": "a.txt", "text": "x"}).to_string(),
                ),
                ("bash".into(), json!({"command": "ls; rm -f b"}).to_string()),
                ("bash".into(), json!({"command": "ls | sort"}).to_string()),
            ],
            ..Default::default()
        }),
        text("Looked.\n\nPlan:\n1. Create `a.txt`\n2. Check it\n"),
        call("write", json!({"path": "a.txt", "text": "x\n"})),
        text("Created it. [DONE:1]"),
        text("Checked. [DONE:2]"),
    ]);
    std::fs::write(f.cwd.join("b"), "keep").unwrap();
    let mut a = f.agent(Budgets::default());
    a.set_plan_mode(true).unwrap();
    let out = a.run("add a.txt").await;
    assert_eq!(out.stop, Stop::Done);
    let r = tool_results(&a);
    assert_eq!(
        r[0],
        (
            "write is off in plan mode. Put the change in your plan instead.".into(),
            true
        )
    );
    assert!(
        r[1].1 && r[1].0.contains("rm isn't on the read-only list"),
        "{}",
        r[1].0
    );
    assert!(!r[2].1);
    assert!(f.cwd.join("b").exists(), "the refused rm never ran");
    let reqs = f.server.chat_requests();
    let tools: Vec<&str> = reqs[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(tools, ["read", "bash"]);
    let user = reqs[0]["messages"][1]["content"].as_str().unwrap();
    assert!(
        user.starts_with("add a.txt\n\n[Plan mode] Explore and plan only"),
        "{user}"
    );
    let steps: Vec<&str> = a.plan().steps.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(steps, ["Create a.txt", "Check it"]);

    let prompt = a.start_plan().unwrap();
    assert!(prompt.contains("\n1. Create a.txt\n2. Check it\n"));
    a.run(&prompt).await;
    assert_eq!(std::fs::read_to_string(f.cwd.join("a.txt")).unwrap(), "x\n");
    assert!(a.plan().executing);
    assert_eq!(a.plan().done_count(), 1);
    let reqs = f.server.chat_requests();
    assert_eq!(
        reqs[2]["tools"].as_array().unwrap().len(),
        4,
        "write and edit are back"
    );
    a.run("check it").await;
    assert_eq!(a.plan().done_count(), 2);
    assert!(!a.plan().executing, "all steps done");
    // The state is in the session: a reopened branch has it.
    let snapshot = a.plan().clone();
    a.restore_plan();
    assert_eq!(*a.plan(), snapshot);
}

/// Turning plan mode off by hand tells the model with the next prompt.
#[tokio::test]
async fn plan_mode_off_says_so_once() {
    let f = Fixture::new(vec![text("p"), text("q"), text("r")]);
    let mut a = f.agent(Budgets::default());
    a.set_plan_mode(true).unwrap();
    a.run("one").await;
    a.set_plan_mode(false).unwrap();
    a.run("two").await;
    a.run("three").await;
    let reqs = f.server.chat_requests();
    let last = |i: usize| {
        reqs[i]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        last(1),
        "two\n\n[Plan mode is off: all tools are available again.]"
    );
    assert_eq!(last(2), "three");
}
