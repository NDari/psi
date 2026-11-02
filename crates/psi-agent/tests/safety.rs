#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! Safety through the real agent loop (psi-plan.md §8, Phase 3): what asks,
//! what is refused, blocked writes, checkpoints with /undo and /rewind, and
//! guards. The model is the replay server; the approver is scripted.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use psi_agent::approval::{
    Approval, ApprovalRequest, Approver, BlockedChoice, BlockedRequest, Headless,
};
use psi_agent::recovery::{rewind, session_changes, undo};
use psi_agent::{Agent, AgentConfig, Budgets, RunOutcome, Safety, Stop};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_safety::always_ask::Rules;
use psi_safety::checkpoint::Checkpoints;
use psi_safety::isolation::{Isolation, SandboxConfig, choose};
use psi_safety::policy::{Mode, Policy};
use psi_session::{EntryBody, SessionFile};
use psi_tools::{ToolCtx, core_tools};
use serde_json::{Value, json};
use tempfile::TempDir;

/// Answers in order; records what it was asked.
#[derive(Default)]
struct Scripted {
    approvals: Mutex<VecDeque<Approval>>,
    blocked: Mutex<VecDeque<BlockedChoice>>,
    asked: Mutex<Vec<ApprovalRequest>>,
    pauses: Mutex<Vec<String>>,
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
        let c = self
            .blocked
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(BlockedChoice::No);
        Box::pin(async move { c })
    }
    fn pause(&self, reason: String) -> BoxFuture<'static, bool> {
        self.pauses.lock().unwrap().push(reason);
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
    tmpdir: PathBuf,
    sessions: PathBuf,
}

impl Fx {
    fn new() -> Self {
        // Not under /tmp: bash's /tmp is private inside the sandbox.
        let t = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let tmp = tempfile::tempdir_in(t).unwrap();
        let root = tmp.path().join("project");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(home.join(".psi")).unwrap();
        Self {
            tmpdir: home.join(".cache/psi/tmp/s"),
            sessions: home.join(".psi/sessions"),
            root,
            home,
            _tmp: tmp,
        }
    }

    fn checkpoints(&self, session: &SessionFile) -> Arc<Checkpoints> {
        let id = session.path().file_stem().unwrap().to_string_lossy();
        Arc::new(Checkpoints::open(&self.home.join(".psi/checkpoints"), &self.root, &id).unwrap())
    }

    fn safety(&self, approver: Arc<dyn Approver>, cp: Option<Arc<Checkpoints>>) -> Safety {
        Safety {
            policy: Policy::new(
                Mode::Auto,
                Rules::shipped(),
                &self.root,
                Some(&self.home),
                &self.tmpdir,
            ),
            checkpoints: cp,
            approver,
            grants: HashSet::new(),
            info: Value::Null,
        }
    }

    fn agent(
        &self,
        server: &ReplayServer,
        session: SessionFile,
        safety: Safety,
        sandbox: bool,
    ) -> Agent {
        let mut ctx = ToolCtx::new(&self.root, &self.tmpdir, 20_000);
        if sandbox {
            let cfg = SandboxConfig {
                root: self.root.clone(),
                git_dirs: vec![],
                cache_dirs: vec![],
                tmpdir: self.tmpdir.clone(),
                network: true,
            };
            let Isolation::On(sb) = choose(true, cfg, Some(&self.home)) else {
                panic!("the sandbox must work for the local check");
            };
            ctx.sandbox = Some(sb);
        }
        let cfg = AgentConfig {
            model: "local".into(),
            max_output: 512,
            n_ctx: 8192,
            enable_thinking: None,
            budgets: Budgets::default(),
            system_prompt: "sys".into(),
            tools: core_tools(false),
            home: Some(self.home.clone()),
        };
        Agent::new(Client::new(&server.base_url()), session, cfg, ctx).with_safety(safety)
    }

    fn session(&self) -> SessionFile {
        SessionFile::create(&self.sessions, &self.root).unwrap()
    }

    fn read(&self, p: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(p)).ok()
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
            } => Some((psi_session::text_of(content), *is_error)),
            _ => None,
        })
        .collect()
}

fn decisions(a: &Agent) -> Vec<String> {
    a.session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::PolicyDecision { decision, .. } => Some(decision.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn headless_denies_always_ask_and_allow_grants() {
    let fx = Fx::new();
    let push = json!({"command": "make build && git push origin main"});
    let server = ReplayServer::start(vec![call("bash", push.clone()), text("ok")]).unwrap();
    let mut a = fx.agent(
        &server,
        fx.session(),
        fx.safety(Arc::new(Headless), None),
        false,
    );
    a.run("ship it").await;
    let r = results(&a);
    assert_eq!(
        r[0],
        (
            "Not run: `git push` is on the always-ask list. This run can't ask the user, so it was denied. Don't retry it in another form; say in your answer what you needed.".into(),
            true
        )
    );
    assert_eq!(decisions(&a), ["denied"]);

    // `--allow git-push` grants it up front.
    let server = ReplayServer::start(vec![call("bash", push), text("ok")]).unwrap();
    let mut safety = fx.safety(Arc::new(Headless), None);
    safety.grants.insert("git push".into());
    let mut a = fx.agent(&server, fx.session(), safety, false);
    a.run("ship it").await;
    assert!(
        !results(&a)[0].0.starts_with("Not run"),
        "{:?}",
        results(&a)
    );
    assert_eq!(decisions(&a), ["granted"]);
}

#[tokio::test]
async fn approvals_once_session_and_deny_with_a_message() {
    let fx = Fx::new();
    let server = ReplayServer::start(vec![
        call(
            "bash",
            json!({"command": "git push", "intent": "publish the fix"}),
        ),
        call("bash", json!({"command": "git -C . push --tags"})),
        call("bash", json!({"command": "kubectl apply -f x"})),
        text("ok"),
    ])
    .unwrap();
    let ap = Arc::new(Scripted::default());
    ap.approvals.lock().unwrap().extend([
        Approval::Session,
        Approval::DenyWith("not on a Friday".into()),
    ]);
    let mut a = fx.agent(&server, fx.session(), fx.safety(ap.clone(), None), false);
    a.run("go").await;
    let asked = ap.asked.lock().unwrap();
    // The session grant covers the second push: two asks, not three.
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[0].intent.as_deref(), Some("publish the fix"));
    assert_eq!(asked[0].keys, ["git push"]);
    assert_eq!(asked[1].keys, ["kubectl"]);
    assert_eq!(decisions(&a), ["approved_session", "granted", "denied"]);
    assert_eq!(
        results(&a)[2],
        ("The user denied this call: not on a Friday".into(), true)
    );
}

#[tokio::test]
async fn file_tools_refuse_psi_config_and_ask_for_unrecoverable_paths() {
    let fx = Fx::new();
    std::fs::write(fx.root.join(".gitignore"), "*.log\n").unwrap();
    let outside = fx.root.parent().unwrap().join("outside.txt");
    let server = ReplayServer::start(vec![
        call(
            "write",
            json!({"path": ".psi/settings.jsonc", "text": "{}"}),
        ),
        call("write", json!({"path": "debug.log", "text": "x"})),
        call(
            "write",
            json!({"path": outside.to_str().unwrap(), "text": "x"}),
        ),
        call("write", json!({"path": "src/ok.py", "text": "x"})),
        text("ok"),
    ])
    .unwrap();
    let session = fx.session();
    let cp = fx.checkpoints(&session);
    let mut a = fx.agent(
        &server,
        session,
        fx.safety(Arc::new(Headless), Some(cp)),
        false,
    );
    a.run("go").await;
    let r = results(&a);
    assert!(r[0].0.contains("psi's own configuration"), "{r:?}");
    assert!(
        r[1].0.starts_with("Not run: debug.log is gitignored"),
        "{r:?}"
    );
    assert!(
        r[2].0
            .contains("is outside the workspace, so checkpoints can't undo it"),
        "{r:?}"
    );
    assert_eq!(r[3], ("Created src/ok.py (1 line).".into(), false));
    assert_eq!(decisions(&a), ["refused", "denied", "denied"]);
    assert!(!fx.root.join(".psi/settings.jsonc").exists());
    assert!(!fx.root.join("debug.log").exists() && !outside.exists());
}

#[tokio::test]
async fn a_symlink_and_dotdot_cannot_reach_psi_config() {
    // Bash in the sandbox may create a symlink to anywhere; a write through
    // it with `..` lands where the OS resolves it (2026-11-01 audit).
    let fx = Fx::new();
    std::fs::create_dir_all(fx.home.join(".cache")).unwrap();
    std::fs::write(fx.home.join(".psi/settings.jsonc"), "orig").unwrap();
    let link = format!(
        "mkdir -p x && ln -s {} x/cfg",
        fx.home.join(".cache").display()
    );
    let server = ReplayServer::start(vec![
        call("bash", json!({"command": link})),
        call(
            "write",
            json!({"path": "x/cfg/../.psi/settings.jsonc", "text": "{}"}),
        ),
        text("ok"),
    ])
    .unwrap();
    let session = fx.session();
    let mut a = fx.agent(&server, session, fx.safety(Arc::new(Headless), None), true);
    a.run("go").await;
    let r = results(&a);
    assert!(!r[0].1, "{r:?}");
    assert!(
        r[1].1 && r[1].0.contains("psi's own configuration"),
        "{r:?}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.home.join(".psi/settings.jsonc")).unwrap(),
        "orig"
    );
}

#[tokio::test]
async fn session_grants_end_with_the_session_but_allow_flags_stay() {
    // --allow git push at the start; `s` on a git stash drop card. A new session
    // keeps the first and asks for git stash drop again.
    let fx = Fx::new();
    let server = ReplayServer::start(vec![
        call("bash", json!({"command": "git stash drop"})),
        call("bash", json!({"command": "git push --dry-run"})),
        text("one"),
        call("bash", json!({"command": "git stash drop"})),
        call("bash", json!({"command": "git push --dry-run"})),
        text("two"),
    ])
    .unwrap();
    let approver = Arc::new(Scripted::default());
    approver
        .approvals
        .lock()
        .unwrap()
        .extend([Approval::Session, Approval::Deny]);
    let mut safety = fx.safety(approver.clone(), None);
    safety.grants.insert("git push".into());
    let mut a = fx.agent(&server, fx.session(), safety, false);
    a.run("go").await;
    a.set_session(fx.session(), None);
    a.run("again").await;
    let asked: Vec<String> = approver
        .asked
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.args["command"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(asked, ["git stash drop", "git stash drop"]);
}

#[tokio::test]
async fn a_failing_checkpoint_makes_writes_ask() {
    let fx = Fx::new();
    let server = ReplayServer::start(vec![
        call("write", json!({"path": "a.txt", "text": "x"})),
        text("ok"),
    ])
    .unwrap();
    let session = fx.session();
    let cp = fx.checkpoints(&session);
    let mut a = fx.agent(
        &server,
        session,
        fx.safety(Arc::new(Headless), Some(cp.clone())),
        false,
    );
    // The shadow repository disappears: the submit capture fails.
    std::fs::remove_dir_all(cp.git_dir()).unwrap();
    a.run("go").await;
    let r = results(&a);
    assert!(r[0].0.contains("checkpoints are failing"), "{r:?}");
    assert!(!fx.root.join("a.txt").exists());
}

#[tokio::test]
async fn blocked_writes_offer_a_rerun() {
    let fx = Fx::new();
    let outside = fx.root.parent().unwrap().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let cmd = json!({"command": format!("echo hi > {}/f", outside.display())});

    // Headless: the call stays failed, with one next step.
    let server = ReplayServer::start(vec![call("bash", cmd.clone()), text("ok")]).unwrap();
    let mut a = fx.agent(
        &server,
        fx.session(),
        fx.safety(Arc::new(Headless), None),
        true,
    );
    a.run("go").await;
    let r = results(&a);
    let want = format!(
        "Blocked: writing {}/f is outside the workspace. Explain to the user what you needed instead of trying variants.",
        outside.canonicalize().unwrap().display()
    );
    assert!(r[0].1 && r[0].0.ends_with(&want), "{r:?}");
    assert_eq!(decisions(&a), ["blocked"]);
    assert!(!outside.join("f").exists());

    // The user reruns it once outside the boundary.
    let server = ReplayServer::start(vec![call("bash", cmd.clone()), text("ok")]).unwrap();
    let ap = Arc::new(Scripted::default());
    ap.blocked
        .lock()
        .unwrap()
        .push_back(BlockedChoice::RerunOutside);
    let mut a = fx.agent(&server, fx.session(), fx.safety(ap, None), true);
    a.run("go").await;
    assert_eq!(
        results(&a)[0],
        (
            "[the user approved running it outside the sandbox]\n(no output)".into(),
            false
        )
    );
    assert_eq!(std::fs::read_to_string(outside.join("f")).unwrap(), "hi\n");
    std::fs::remove_file(outside.join("f")).unwrap();

    // Or adds the directory for the session.
    let server = ReplayServer::start(vec![
        call("bash", cmd.clone()),
        call("bash", cmd),
        text("ok"),
    ])
    .unwrap();
    let ap = Arc::new(Scripted::default());
    ap.blocked.lock().unwrap().push_back(BlockedChoice::AddDir);
    let mut a = fx.agent(&server, fx.session(), fx.safety(ap, None), true);
    a.run("go").await;
    let r = results(&a);
    assert!(r[0].0.starts_with("[the user made ") && !r[0].1, "{r:?}");
    // The second call runs inside, now that the directory is writable.
    assert_eq!(r[1], ("(no output)".into(), false));
    assert_eq!(decisions(&a), ["dir_added"]);
}

#[tokio::test]
async fn undo_and_rewind_restore_edits_and_bash_changes() {
    let fx = Fx::new();
    std::fs::write(fx.root.join("c.txt"), "c\n").unwrap();
    let server = ReplayServer::start(vec![
        call("write", json!({"path": "a.txt", "text": "one\n"})),
        text("run 1 done"),
        call(
            "edit",
            json!({"path": "a.txt", "old_text": "one", "new_text": "two"}),
        ),
        call("bash", json!({"command": "echo b > b.txt; rm c.txt"})),
        text("run 2 done"),
    ])
    .unwrap();
    let session = fx.session();
    let cp = fx.checkpoints(&session);
    let mut a = fx.agent(
        &server,
        session,
        fx.safety(Arc::new(Headless), Some(cp.clone())),
        true,
    );
    assert_eq!(a.run("first").await.stop, Stop::Done);
    // The user's own edit between prompts is captured at the next submit.
    std::fs::write(fx.root.join("mine.txt"), "mine\n").unwrap();
    let out: RunOutcome = a.run("second").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", results(&a));
    assert_eq!(fx.read("a.txt").as_deref(), Some("two\n"));
    assert_eq!(fx.read("b.txt").as_deref(), Some("b\n"));
    assert_eq!(fx.read("c.txt"), None);

    let changes = session_changes(a.session(), &cp).unwrap();
    let sources: Vec<&str> = changes.iter().map(|c| c.source.as_str()).collect();
    assert_eq!(sources.len(), 4, "{sources:?}");
    assert!(sources[0].starts_with("by write call"), "{sources:?}");
    assert_eq!(sources[1], "outside the agent");
    assert!(sources[2].starts_with("by edit call"), "{sources:?}");
    assert!(sources[3].starts_with("by bash call"), "{sources:?}");
    assert!(changes[3].diff.contains("+b"), "{}", changes[3].diff);

    // /undo: back to the start of run 2 (the user's file stays), then run 1.
    undo(a.session_mut(), &cp).unwrap().unwrap();
    assert_eq!(fx.read("a.txt").as_deref(), Some("one\n"));
    assert_eq!(fx.read("b.txt"), None);
    assert_eq!(fx.read("c.txt").as_deref(), Some("c\n"));
    assert_eq!(fx.read("mine.txt").as_deref(), Some("mine\n"));
    undo(a.session_mut(), &cp).unwrap().unwrap();
    assert_eq!(fx.read("a.txt"), None);
    assert_eq!(fx.read("mine.txt"), None);

    // /rewind to the second prompt: files as they were when it was sent,
    // and the conversation continues from there.
    let second = a
        .session()
        .entries()
        .iter()
        .find(|e| matches!(&e.body, EntryBody::Message { content, .. } if psi_session::text_of(content) == "second"))
        .unwrap()
        .id
        .clone();
    rewind(a.session_mut(), &cp, &second).unwrap().unwrap();
    assert_eq!(fx.read("a.txt").as_deref(), Some("one\n"));
    assert_eq!(fx.read("mine.txt").as_deref(), Some("mine\n"));
    assert_eq!(fx.read("b.txt"), None);
    let branch = a.session().branch();
    let last_messages: Vec<String> = branch
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::Message { content, .. } => Some(psi_session::text_of(content)),
            _ => None,
        })
        .collect();
    assert_eq!(last_messages.last().map(String::as_str), Some("second"));
    assert!(!last_messages.iter().any(|m| m == "run 2 done"));
    // The rewind is checkpointed too: a capture now stores nothing new.
    assert!(!cp.capture("check").unwrap().changed);
}

#[tokio::test]
async fn repeated_failures_nudge_then_pause() {
    let fx = Fx::new();
    let replies = (0..6)
        .map(|_| {
            call(
                "bash",
                json!({"command": "ls tests/test_app.py", "intent": "find the tests"}),
            )
        })
        .collect();
    let server = ReplayServer::start(replies).unwrap();
    let ap = Arc::new(Scripted::default());
    let mut a = fx.agent(&server, fx.session(), fx.safety(ap.clone(), None), false);
    let out = a.run("test").await;
    assert_eq!(
        out.stop,
        Stop::Guard("the same call failed 5 times in a row".into())
    );
    let r = results(&a);
    assert_eq!(r.len(), 5);
    assert!(
        r[2].0.ends_with(
            "This call failed the same way 3 times; change the call or try another approach."
        ),
        "{r:?}"
    );
    assert!(!r[1].0.contains("same way"));
    assert_eq!(
        *ap.pauses.lock().unwrap(),
        ["the same call failed 5 times in a row"]
    );
}
