#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! Safety through the real agent loop (design-going-forward.md, goal 1):
//! what is refused, blocked writes, hidden credentials, checkpoints with
//! /undo and /rewind, and guards. The model is the replay server.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use psi_agent::recovery::{rewind, session_changes, undo};
use psi_agent::{Agent, AgentConfig, Budgets, RunOutcome, Safety, Stop};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_safety::checkpoint::Checkpoints;
use psi_safety::isolation::{Isolation, SandboxConfig, choose};
use psi_safety::policy::Policy;
use psi_safety::refuse::Rules;
use psi_session::{EntryBody, SessionFile};
use psi_tools::{ToolCtx, core_tools};
use serde_json::{Value, json};
use tempfile::TempDir;

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

    fn safety(&self, cp: Option<Arc<Checkpoints>>) -> Safety {
        Safety {
            policy: Policy::new(Rules::shipped(), &self.root, Some(&self.home), &self.tmpdir),
            checkpoints: cp,
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
        self.agent_with(server, session, safety, sandbox, Budgets::default())
    }

    fn agent_with(
        &self,
        server: &ReplayServer,
        session: SessionFile,
        safety: Safety,
        sandbox: bool,
        budgets: Budgets,
    ) -> Agent {
        let mut ctx = ToolCtx::new(&self.root, &self.tmpdir, 20_000);
        if sandbox {
            let cfg = SandboxConfig {
                root: self.root.clone(),
                git_dirs: vec![],
                cache_dirs: vec![],
                tmpdir: self.tmpdir.clone(),
                network: true,
                hidden: psi_safety::secrets::hidden_paths(&self.home, None),
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
            sampling: Default::default(),
            budgets,
            system_prompt: "sys".into(),
            tools: core_tools(),
            home: Some(self.home.clone()),
            attended: false,
            task_budgets: Budgets::default(),
        };
        Agent::new(Client::new(&server.base_url()), session, cfg, ctx).with_safety(safety)
    }

    fn session(&self) -> SessionFile {
        SessionFile::create(&self.sessions, &self.root).unwrap()
    }

    fn read(&self, p: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(p)).ok()
    }

    /// Runs git in the project, as the user would.
    fn git(&self, args: &[&str]) -> String {
        let out = psi_safety::git::host_git()
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "init.defaultBranch=main"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .current_dir(&self.root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
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

/// Nothing asks: a listed command is refused, and the model is told what
/// to do.
#[tokio::test]
async fn listed_commands_are_refused_and_nothing_asks() {
    let fx = Fx::new();
    let push = json!({"command": "make build && git push origin main"});
    let server = ReplayServer::start(vec![call("bash", push), text("ok")]).unwrap();
    let mut a = fx.agent(&server, fx.session(), fx.safety(None), false);
    assert_eq!(a.run("ship it").await.stop, Stop::Done);
    assert_eq!(
        results(&a)[0],
        (
            "Not run: psi refuses `git push`, and the command contains it. Leave it to the user: do the rest without it, and say in your answer that it still has to be done.".into(),
            true
        )
    );
    assert_eq!(decisions(&a), ["refused"]);
}

/// A `/label` typed while the run works marks the entry that was latest
/// then and is written before the next entry; the model never sees it. In
/// the cog trial (2026-11-05) the owner's label had to wait for the end of
/// the turn.
#[tokio::test]
async fn a_label_typed_during_a_run_marks_the_latest_entry() {
    let fx = Fx::new();
    let server = ReplayServer::start(vec![
        call("bash", json!({"command": "sleep 0.3"})),
        text("ok"),
    ])
    .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut a = fx
        .agent(&server, fx.session(), fx.safety(None), false)
        .with_events(tx);
    // Typed when the call starts, while it runs.
    let labels = a.queues().labels.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if matches!(ev, psi_agent::AgentEvent::ToolStart { .. }) {
                labels
                    .lock()
                    .unwrap()
                    .push_back("typed while working".into());
            }
        }
    });
    a.run("go").await;
    let entries = a.session().entries();
    let at = entries
        .iter()
        .position(|e| matches!(e.body, EntryBody::Label { .. }))
        .expect("a label entry");
    let EntryBody::Label { target_id, label } = &entries[at].body else {
        unreachable!()
    };
    assert_eq!(label.as_deref(), Some("typed while working"));
    // The latest entry when it was typed: the call, before its result.
    assert_eq!(target_id, &entries[at - 1].id);
    assert!(
        !entries[..at]
            .iter()
            .any(|e| matches!(e.body, EntryBody::ToolResult { .. }))
    );
    // Not part of what the model is sent.
    let last = server.chat_requests().last().cloned().unwrap();
    assert!(!last.to_string().contains("typed while working"));
}

/// Qwen3.8 27B, `sh_precommit` (code eval 2026-11-05, step 18): a quoting
/// slip. It runs, and bash names the error: a refusal could only guess,
/// and the parser also rejects some valid bash.
#[tokio::test]
async fn a_command_that_does_not_parse_runs_and_bash_names_the_error() {
    let fx = Fx::new();
    let cmd = json!({"command": "t=$(printf '\\t')\necho '--- content of staged b.py:'\ngit cat-file -p :'we'\"$t\"dir/b.py'; echo \"<<<end\"\necho '--- diff output via git diff --cached for b.py:'\ngit diff --cached -- 'we'\"$t\"dir/b.py' | cat -A\necho '--- diff with -U0:'\ngit diff --cached -U0 -- 'we'\"$t\"dir/b.py' | cat -A"});
    let server = ReplayServer::start(vec![call("bash", cmd), text("ok")]).unwrap();
    let mut a = fx.agent(&server, fx.session(), fx.safety(None), false);
    a.run("test the hook").await;
    let r = results(&a);
    assert!(r[0].1, "{r:?}");
    assert!(
        r[0].0.contains("unexpected EOF while looking for matching"),
        "{r:?}"
    );
    assert!(decisions(&a).is_empty());
}

#[tokio::test]
async fn file_tools_stay_inside_the_workspace() {
    let fx = Fx::new();
    std::fs::write(fx.root.join(".gitignore"), "*.log\n.env\n").unwrap();
    std::fs::write(fx.root.join(".env"), "KEY=1\n").unwrap();
    let outside = fx.root.parent().unwrap().join("outside.txt");
    let user_settings = fx.home.join(".psi/settings.jsonc");
    let server = ReplayServer::start(vec![
        call(
            "write",
            json!({"path": outside.to_str().unwrap(), "text": "x"}),
        ),
        call(
            "write",
            json!({"path": user_settings.to_str().unwrap(), "text": "{}"}),
        ),
        // Inside: the project's own settings (they can't loosen safety),
        // and gitignored files, which checkpoints capture when small.
        call(
            "write",
            json!({"path": ".psi/settings.jsonc", "text": "{}"}),
        ),
        call("write", json!({"path": "debug.log", "text": "x"})),
        call("bash", json!({"command": "echo > .env"})),
        text("ok"),
    ])
    .unwrap();
    let session = fx.session();
    let cp = fx.checkpoints(&session);
    let mut a = fx.agent(&server, session, fx.safety(Some(cp.clone())), false);
    a.run("go").await;
    let r = results(&a);
    let want = format!(
        "Not written: {} is outside the workspace ({}), and psi only changes files inside it. If the task needs it, say so in your answer.",
        outside.display(),
        fx.root.canonicalize().unwrap().display()
    );
    assert_eq!(r[0], (want, true));
    assert!(
        r[1].1 && r[1].0.contains("is outside the workspace"),
        "{r:?}"
    );
    assert!(!r[2].1 && !r[3].1 && !r[4].1, "{r:?}");
    assert_eq!(decisions(&a), ["refused", "refused"]);
    assert!(!outside.exists() && !user_settings.exists());
    assert_eq!(fx.read(".env").as_deref(), Some("\n"));
    // /undo brings the clobbered .env back and takes the new files away.
    undo(a.session_mut(), &cp).unwrap().unwrap();
    assert_eq!(fx.read(".env").as_deref(), Some("KEY=1\n"));
    assert_eq!(fx.read("debug.log"), None);
    assert_eq!(fx.read(".psi/settings.jsonc"), None);
}

/// Credentials are out of reach both ways: read refuses them, and bash
/// inside the boundary finds the directory empty.
#[tokio::test]
async fn credentials_cannot_be_read() {
    let fx = Fx::new();
    std::fs::create_dir_all(fx.home.join(".ssh")).unwrap();
    std::fs::write(fx.home.join(".ssh/id_ed25519"), "secret-key\n").unwrap();
    std::fs::write(fx.home.join(".git-credentials"), "secret-token\n").unwrap();
    let key = fx.home.join(".ssh/id_ed25519");
    let token = fx.home.join(".git-credentials");
    let server = ReplayServer::start(vec![
        call("read", json!({"path": key.to_str().unwrap()})),
        call(
            "bash",
            json!({"command": format!("cat {} {}", key.display(), token.display())}),
        ),
        text("ok"),
    ])
    .unwrap();
    let safety = fx.safety(None);
    let mut a = fx.agent(&server, fx.session(), safety, true);
    a.run("go").await;
    let r = results(&a);
    assert_eq!(
        r[0],
        (
            format!(
                "Not read: {} holds credentials, which psi keeps out of reach. Do without it.",
                key.display()
            ),
            true
        )
    );
    assert!(r[1].1 && !r[1].0.contains("secret"), "{r:?}");
    assert_eq!(decisions(&a), ["refused"]);
}

/// A run is bounded: at a limit it stops with the reason and never waits
/// for an answer (design-going-forward.md).
#[tokio::test]
async fn a_limit_stops_the_run_with_the_reason() {
    let fx = Fx::new();
    let ls = || call("bash", json!({"command": "true"}));
    for (budgets, want, turns) in [
        (
            Budgets {
                max_turns: 2,
                ..Budgets::default()
            },
            "reached the limit of 2 model requests in one run",
            2,
        ),
        (
            Budgets {
                max_tool_calls: 3,
                ..Budgets::default()
            },
            "reached the limit of 3 tool calls in one run",
            3,
        ),
    ] {
        let server = ReplayServer::start(vec![ls(), ls(), ls(), ls(), text("done")]).unwrap();
        let mut a = fx.agent_with(&server, fx.session(), fx.safety(None), false, budgets);
        let out = a.run("go").await;
        assert_eq!(out.stop, Stop::Budget(want.into()));
        assert_eq!(out.turns, turns);
    }
}

#[tokio::test]
async fn a_symlink_and_dotdot_cannot_get_out_of_the_workspace() {
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
    let mut a = fx.agent(&server, session, fx.safety(None), true);
    a.run("go").await;
    let r = results(&a);
    assert!(!r[0].1, "{r:?}");
    assert!(
        r[1].1 && r[1].0.contains("is outside the workspace"),
        "{r:?}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.home.join(".psi/settings.jsonc")).unwrap(),
        "orig"
    );
}

#[tokio::test]
async fn a_failing_checkpoint_is_recorded_and_the_run_goes_on() {
    let fx = Fx::new();
    let server = ReplayServer::start(vec![
        call("write", json!({"path": "a.txt", "text": "x"})),
        text("ok"),
    ])
    .unwrap();
    let session = fx.session();
    let cp = fx.checkpoints(&session);
    let mut a = fx.agent(&server, session, fx.safety(Some(cp.clone())), false);
    // The shadow repository disappears: the captures fail.
    std::fs::remove_dir_all(cp.git_dir()).unwrap();
    assert_eq!(a.run("go").await.stop, Stop::Done);
    assert_eq!(fx.read("a.txt").as_deref(), Some("x"));
    let events = guard_events(&a);
    assert!(
        events.iter().any(|(g, m)| g == "checkpoint_failed"
            && m.starts_with(
                "the checkpoint failed, so changes since the last one can't be undone"
            )),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_blocked_write_says_so() {
    let fx = Fx::new();
    let outside = fx.root.parent().unwrap().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let cmd = json!({"command": format!("echo hi > {}/f", outside.display())});
    {
        let server = ReplayServer::start(vec![call("bash", cmd.clone()), text("ok")]).unwrap();
        let mut a = fx.agent(&server, fx.session(), fx.safety(None), true);
        a.run("go").await;
        let r = results(&a);
        let want = format!(
            "Blocked: writing {}/f is outside the workspace. Explain to the user what you needed instead of trying variants.",
            outside.canonicalize().unwrap().display()
        );
        assert!(r[0].1 && r[0].0.ends_with(&want), "{r:?}");
        assert_eq!(decisions(&a), ["blocked"]);
        assert!(!outside.join("f").exists());
    }
}

/// The model may commit, branch and rewrite history: /undo puts the files
/// and the branch tips back (design-going-forward.md, goal 1).
#[tokio::test]
async fn undo_puts_back_the_branch_tips_after_a_rewrite() {
    let fx = Fx::new();
    fx.git(&["init", "-q"]);
    std::fs::write(fx.root.join("a.txt"), "one\n").unwrap();
    fx.git(&["add", "a.txt"]);
    fx.git(&["commit", "-q", "-m", "first"]);
    std::fs::write(fx.root.join("a.txt"), "two\n").unwrap();
    fx.git(&["commit", "-q", "-am", "second"]);
    let second = fx.git(&["rev-parse", "HEAD"]);
    // The user's own uncommitted work.
    std::fs::write(fx.root.join("a.txt"), "mine\n").unwrap();
    let rewrite = "git -c user.name=m -c user.email=m@m commit -qam wip \
        && git reset -q --hard HEAD~2 && git checkout -q -b scratch && git branch -D main";
    let server = ReplayServer::start(vec![
        call("bash", json!({"command": rewrite})),
        text("done"),
    ])
    .unwrap();
    let session = fx.session();
    let cp = fx.checkpoints(&session);
    let mut a = fx.agent(&server, session, fx.safety(Some(cp.clone())), false);
    assert_eq!(a.run("tidy up").await.stop, Stop::Done);
    let r = results(&a);
    assert!(!r[0].1, "{r:?}");
    assert_eq!(fx.read("a.txt").as_deref(), Some("one\n"));
    assert_eq!(fx.git(&["branch", "--list", "main"]), "");

    undo(a.session_mut(), &cp).unwrap().unwrap();
    assert_eq!(fx.git(&["rev-parse", "main"]), second);
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(fx.read("a.txt").as_deref(), Some("mine\n"));
    assert_eq!(fx.git(&["status", "--porcelain"]), "M a.txt");
}

/// A commit changes no file, only the branch tip: /undo still finds it.
#[tokio::test]
async fn undo_takes_back_a_commit_that_changed_no_file() {
    let fx = Fx::new();
    fx.git(&["init", "-q"]);
    std::fs::write(fx.root.join("a.txt"), "one\n").unwrap();
    fx.git(&["add", "a.txt"]);
    fx.git(&["commit", "-q", "-m", "first"]);
    let first = fx.git(&["rev-parse", "HEAD"]);
    std::fs::write(fx.root.join("a.txt"), "mine\n").unwrap();
    let commit = "git -c user.name=m -c user.email=m@m commit -qam wip";
    let server = ReplayServer::start(vec![
        call("bash", json!({"command": commit})),
        text("committed"),
    ])
    .unwrap();
    let session = fx.session();
    let cp = fx.checkpoints(&session);
    let mut a = fx.agent(&server, session, fx.safety(Some(cp.clone())), false);
    assert_eq!(a.run("commit it").await.stop, Stop::Done);
    assert_ne!(fx.git(&["rev-parse", "HEAD"]), first);
    let r = undo(a.session_mut(), &cp).unwrap().unwrap();
    assert_eq!(r.refs, ["refs/heads/main"]);
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), first);
    assert_eq!(fx.git(&["status", "--porcelain"]), "M a.txt");
    assert!(undo(a.session_mut(), &cp).unwrap().is_none());
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
    let mut a = fx.agent(&server, session, fx.safety(Some(cp.clone())), true);
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

/// The guard events a run recorded, as (guard, message).
fn guard_events(a: &Agent) -> Vec<(String, String)> {
    a.session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::GuardEvent { guard, message, .. } => Some((guard.clone(), message.clone())),
            _ => None,
        })
        .collect()
}

/// Two exact commands in turn, each working and changing nothing: the
/// shape of the 2026-11-06 TUI rerun's loop (two `grep`s, 623 calls, 103
/// nudges, stopped by hand after 106 minutes).
fn two_commands_in_turn(n: usize) -> Vec<Reply> {
    (0..n)
        .map(|i| {
            let cmd = if i % 2 == 0 { "echo a" } else { "echo b" };
            // The model rewords the intent; the guard leaves it out.
            call(
                "bash",
                json!({"command": cmd, "intent": format!("look, {i}")}),
            )
        })
        .collect()
}

const NO_PROGRESS_PAUSE: &str = "the last 12 turns repeated earlier calls and changed no file";

#[tokio::test]
async fn a_run_goes_on_once_with_the_note_and_stops_the_second_time() {
    let fx = Fx::new();
    let server = ReplayServer::start(two_commands_in_turn(40)).unwrap();
    let safety = fx.safety(None);
    let mut a = fx.agent(&server, fx.session(), safety, false);
    let out = a.run("look around").await;
    // The note at the first stop (14 calls), the stop itself at the
    // second (12 more).
    assert_eq!(out.stop, Stop::Guard(NO_PROGRESS_PAUSE.into()));
    assert_eq!(out.tool_calls, 26);
    let reqs = server.chat_requests();
    // The note tells the model which calls it repeated: as things were,
    // the next call repeated in 40 of 42 samples (experiments.md, 2026-11-09).
    assert_eq!(
        reqs[14]["messages"].as_array().unwrap().last().unwrap(),
        &json!({"role": "user", "content": "[Note from psi] You made the same calls again and again, the last 12 turns in a row, and no file changed. The calls:\n- bash: echo a\n- bash: echo b\nDon't make them again: use what you have, do something different, or answer."})
    );
    let notes = |v: &Value| v.to_string().matches("[Note from psi]").count();
    assert_eq!(
        (notes(&reqs[13]), notes(&reqs[14]), notes(&reqs[25])),
        (0, 1, 1)
    );
    let events = guard_events(&a);
    assert_eq!(
        events.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>()[1..],
        [
            "stopped: the last 12 turns repeated earlier calls and changed no file",
            "the run goes on once, with a note naming the repeated calls",
            // After the note the nudge comes again first.
            "The last 6 turns repeated earlier calls and changed no file. Try a different approach, or answer if you are done.",
            "stopped: the last 12 turns repeated earlier calls and changed no file",
        ]
    );
}

#[tokio::test]
async fn a_run_that_leaves_the_loop_after_the_note_finishes() {
    let fx = Fx::new();
    let mut replies = two_commands_in_turn(14);
    replies.push(call(
        "write",
        json!({"path": "notes.txt", "text": "found it\n"}),
    ));
    replies.push(text("done"));
    let server = ReplayServer::start(replies).unwrap();
    let safety = fx.safety(None);
    let mut a = fx.agent(&server, fx.session(), safety, false);
    let out = a.run("look around").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    assert_eq!(fx.read("notes.txt").as_deref(), Some("found it\n"));
}

#[tokio::test]
async fn repeated_failures_nudge_then_stop() {
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
    let mut a = fx.agent(&server, fx.session(), fx.safety(None), false);
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
}
