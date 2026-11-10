#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! A plan carried out through the real agent loop (design-going-forward.md,
//! "The shape"): one task per open item, each in a fresh context, with the
//! harness's check and report at the end, and the tick in the plan file.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use psi_agent::{Agent, AgentConfig, AgentEvent, Budgets, PostEditCheck, Safety};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_safety::checkpoint::Checkpoints;
use psi_session::{EntryBody, SessionFile};
use psi_tools::{ToolCtx, core_tools};
use serde_json::{Value, json};
use tempfile::TempDir;

const PLAN: &str = "# Greeting\n\nTwo files.\n\n- [ ] Write hello.txt\n- [ ] Write bye.txt\n";

struct Fx {
    _tmp: TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let t = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let tmp = tempfile::tempdir_in(t).unwrap();
        let root = tmp.path().join("project");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(home.join(".psi")).unwrap();
        std::fs::write(root.join("plan.md"), PLAN).unwrap();
        Self {
            root,
            home,
            _tmp: tmp,
        }
    }

    fn agent(&self, server: &ReplayServer, task_budgets: Budgets) -> Agent {
        let session = SessionFile::create(&self.home.join(".psi/sessions"), &self.root).unwrap();
        let id = session.path().file_stem().unwrap().to_string_lossy();
        let cp = Checkpoints::open(&self.home.join(".psi/checkpoints"), &self.root, &id).unwrap();
        let tmpdir = self.home.join("tmp");
        let mut safety = Safety::headless(&self.root, Some(&self.home), &tmpdir);
        safety.checkpoints = Some(Arc::new(cp));
        let cfg = AgentConfig {
            model: "local".into(),
            max_output: 512,
            n_ctx: 32_768,
            enable_thinking: None,
            // The chat's own limits: none, as in the TUI.
            budgets: Budgets {
                max_turns: u32::MAX,
                max_tool_calls: u32::MAX,
                ..Budgets::default()
            },
            task_budgets,
            system_prompt: "sys".into(),
            tools: core_tools(),
            home: Some(self.home.clone()),
            attended: true,
        };
        let ctx = ToolCtx::new(&self.root, &tmpdir, 20_000);
        Agent::new(Client::new(&server.base_url()), session, cfg, ctx).with_safety(safety)
    }

    fn plan(&self) -> String {
        std::fs::read_to_string(self.root.join("plan.md")).unwrap()
    }
}

fn write(path: &str, text: &str) -> Reply {
    Reply::Chat(ScriptedReply::call(
        "write",
        json!({"path": path, "text": text}),
    ))
}

fn text(t: &str) -> Reply {
    Reply::Chat(ScriptedReply::text(t))
}

fn customs(a: &Agent, kind: &str) -> Vec<Value> {
    a.session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::Custom { custom_type, data } if custom_type == kind => Some(data.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_plan_runs_one_item_at_a_time_each_in_a_fresh_context() {
    let fx = Fx::new();
    let server = ReplayServer::start(vec![
        text("Let's plan."),
        write("hello.txt", "hello\n"),
        text("I wrote hello.txt, PELICAN."),
        write("bye.txt", "bye\n"),
        text("I wrote bye.txt."),
    ])
    .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut a = fx.agent(&server, Budgets::default()).with_events(tx);
    // The planning conversation before it.
    a.run("We talked about SECRET-PLANNING-CHAT.").await;
    let out = a.run_plan(Path::new("plan.md")).await;
    assert!(out.complete, "{}", out.end);
    assert_eq!(out.end, "all 2 items of plan.md are done");
    assert_eq!(out.open, 0);
    assert_eq!(
        fx.plan(),
        PLAN.replace("- [ ]", "- [x]"),
        "the harness ticked both"
    );

    // Each task's context: the system prompt and the harness's prompt.
    // Nothing of the chat or of the task before it, in the model's words.
    let reqs = server.chat_requests();
    assert_eq!(reqs.len(), 5);
    let first = reqs[1]["messages"].as_array().unwrap();
    assert_eq!(first.len(), 2);
    let prompt = first[1]["content"].as_str().unwrap();
    assert!(prompt.starts_with("[Task from psi] Carry out one item"));
    assert!(prompt.contains("The plan (plan.md):\n# Greeting"));
    assert!(prompt.contains("\nNone in this session.\n\nYour item: Write hello.txt\n"));
    assert!(!reqs[1].to_string().contains("SECRET-PLANNING-CHAT"));
    let second = reqs[3]["messages"].as_array().unwrap();
    assert_eq!(second.len(), 2);
    let prompt = second[1]["content"].as_str().unwrap();
    // The plan as it is now, and the record the harness kept.
    assert!(prompt.contains("- [x] Write hello.txt\n- [ ] Write bye.txt"));
    assert!(prompt.contains(
        "as psi recorded them:\n- Write hello.txt: changed A hello.txt; no check\n\nYour item: Write bye.txt\n"
    ));
    assert!(!reqs[3].to_string().contains("PELICAN"));

    // The reports, from the checkpoints.
    let r = &out.reports;
    assert_eq!(r.len(), 2);
    assert_eq!(
        (r[0].n, r[0].total, r[0].item.as_str()),
        (1, 2, "Write hello.txt")
    );
    assert_eq!(r[0].changed, ["A hello.txt"]);
    assert_eq!(
        r[1].changed,
        ["A bye.txt"],
        "the tick is not the task's change"
    );
    assert!(
        r.iter()
            .all(|r| r.ticked && r.stop == "done" && r.check.is_none())
    );
    assert_eq!((r[0].requests, r[0].tool_calls), (2, 1));
    assert!(r[0].from.is_some() && r[0].from != r[0].to);
    // In the session, and as events.
    assert_eq!(customs(&a, "task_start").len(), 2);
    let recorded = customs(&a, "task_report");
    assert_eq!(recorded[1], serde_json::to_value(&r[1]).unwrap());
    drop(a);
    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            AgentEvent::TaskStart { n, total, item } => {
                events.push(format!("start {n}/{total} {item}"))
            }
            AgentEvent::TaskEnd { report } => events.push(format!("end {}", report.n)),
            AgentEvent::PlanEnd { done, open, reason } => {
                events.push(format!("plan {done} {open} {reason}"));
            }
            _ => {}
        }
    }
    assert_eq!(
        events,
        [
            "start 1/2 Write hello.txt",
            "end 1",
            "start 2/2 Write bye.txt",
            "end 2",
            "plan 2 0 all 2 items of plan.md are done"
        ]
    );
}

#[tokio::test]
async fn a_failed_check_leaves_the_item_open_and_stops_the_plan() {
    let fx = Fx::new();
    // The model says it is done and ticks the box itself; the harness
    // runs the check, which fails.
    let ticked = PLAN.replacen("- [ ]", "- [x]", 1);
    let server = ReplayServer::start(vec![
        write("hello.txt", "hello\n"),
        write("plan.md", &ticked),
        text("All done, the check passes."),
        // The second try does what the check wants.
        write("ok.txt", "ok\n"),
        text("Done."),
        write("bye.txt", "bye\n"),
        text("Done."),
    ])
    .unwrap();
    let check = PostEditCheck {
        command: "test -f ok.txt || { echo 'ok.txt is missing'; exit 1; }".into(),
        timeout_s: 10,
    };
    let mut a = fx
        .agent(&server, Budgets::default())
        .with_post_edit_check(Some(check));
    let out = a.run_plan(Path::new("plan.md")).await;
    assert!(!out.complete);
    assert_eq!(out.end, "stopped at item 1 of 2: the check failed");
    assert_eq!(out.open, 2);
    assert_eq!(server.remaining(), 4, "the second item was not started");
    let r = &out.reports[0];
    assert!(!r.ticked && r.stop == "done");
    let c = r.check.as_ref().unwrap();
    assert!(!c.passed);
    assert_eq!(c.output, "ok.txt is missing\n[exit code 1]");
    assert_eq!(
        r.lines()[3..6],
        [
            "  check: `test -f ok.txt || { echo 'ok.txt is missing'; exit 1; }` failed",
            "    ok.txt is missing",
            "    [exit code 1]"
        ]
    );
    // The model's own tick is taken back.
    assert_eq!(fx.plan(), PLAN);

    // Run again: the same item first, then the next.
    let out = a.run_plan(Path::new("plan.md")).await;
    assert!(out.complete, "{}", out.end);
    assert!(out.reports[0].check.as_ref().unwrap().passed);
    assert_eq!(
        out.reports[0].lines().last().unwrap(),
        "  ticked in plan.md"
    );
    assert_eq!(fx.plan(), PLAN.replace("- [ ]", "- [x]"));
}

#[tokio::test]
async fn a_task_that_reaches_a_limit_is_left_open_with_the_reason() {
    let fx = Fx::new();
    let server = ReplayServer::start(vec![
        write("a.txt", "1\n"),
        write("a.txt", "2\n"),
        write("a.txt", "3\n"),
        text("chat still works"),
    ])
    .unwrap();
    let limits = Budgets {
        max_turns: 2,
        ..Budgets::default()
    };
    let mut a = fx.agent(&server, limits);
    let out = a.run_plan(Path::new("plan.md")).await;
    assert_eq!(
        out.end,
        "stopped at item 1 of 2: the task did not end by itself"
    );
    let r = &out.reports[0];
    assert_eq!(r.stop, "budget");
    assert_eq!(
        r.reason.as_deref(),
        Some("reached the limit of 2 model requests in one run")
    );
    assert_eq!(r.check, None);
    assert_eq!(r.changed, ["A a.txt"]);
    assert_eq!(fx.plan(), PLAN);
    // The limits were the task's: the chat has none again.
    assert_eq!(a.run("go on").await.turns, 2);
}

#[tokio::test]
async fn a_plan_without_a_checklist_says_what_one_looks_like() {
    let fx = Fx::new();
    std::fs::write(fx.root.join("plan.md"), "# Plan\n\nDo things.\n").unwrap();
    let server = ReplayServer::start(vec![]).unwrap();
    let mut a = fx.agent(&server, Budgets::default());
    let out = a.run_plan(Path::new("plan.md")).await;
    assert_eq!(
        out.end,
        "plan.md has no checklist items (lines like `- [ ] add the parser`)"
    );
    let out = a.run_plan(Path::new("missing.md")).await;
    assert!(
        out.end.starts_with("cannot read missing.md: "),
        "{}",
        out.end
    );
    assert!(out.reports.is_empty());
}
