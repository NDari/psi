#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! Real model mistakes, replayed (unhappy paths, batch 4): responses copied
//! byte for byte from small-model eval trials (`just record-fixture`, see
//! fixtures/recorded/README.md) and sent to today's Psi through the replay
//! server, so the harness is tested against what models actually send, not
//! what a test author imagines.

use std::path::{Path, PathBuf};
use std::process::Command;

use psi_agent::{Agent, AgentConfig, Budgets, RunOutcome, Stop};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_session::{EntryBody, SessionFile, text_of};
use psi_tools::{ToolCtx, core_tools};
use serde_json::Value;
use tempfile::TempDir;

/// The recorded responses of fixture `name`, as they were. Paths in them
/// can't be rewritten (arguments stream a token at a time), so the tests'
/// workspace ends in `work/project` like the trials' `/work/project`.
fn recorded(name: &str) -> Vec<Reply> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/recorded")
        .join(format!("{name}.jsonl"));
    std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            Reply::recorded(
                v["status"].as_u64().unwrap() as u16,
                v["body"].as_str().unwrap(),
            )
        })
        .collect()
}

struct Fx {
    _tmp: TempDir,
    cwd: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let t = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let tmp = tempfile::tempdir_in(t).unwrap();
        let cwd = tmp.path().join("work/project");
        std::fs::create_dir_all(&cwd).unwrap();
        Self { cwd, _tmp: tmp }
    }

    /// A headless agent (asks are denied) on `replies`.
    async fn run(&self, replies: Vec<Reply>, prompt: &str) -> (RunOutcome, Agent, ReplayServer) {
        let server = ReplayServer::start(replies).unwrap();
        let session = SessionFile::create(&self.cwd.join("../../sessions"), &self.cwd).unwrap();
        let cfg = AgentConfig {
            model: "local".into(),
            max_output: 8192,
            n_ctx: 65_536,
            enable_thinking: None,
            budgets: Budgets::default(),
            system_prompt: "sys".into(),
            tools: core_tools(false),
            home: None,
        };
        let ctx = ToolCtx::new(&self.cwd, &self.cwd.join("../../tmp"), 20_000);
        let mut a = Agent::new(Client::new(&server.base_url()), session, cfg, ctx);
        let out = a.run(prompt).await;
        (out, a, server)
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A git repository with one commit.
    fn repo(&self) {
        std::fs::write(self.cwd.join("README.md"), "# demo\n").unwrap();
        std::fs::write(self.cwd.join("changelog.sh"), "#!/bin/bash\n").unwrap();
        self.git(&["init", "-q"]);
        self.git(&["add", "-A"]);
        self.git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "init",
        ]);
    }
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

/// Qwen 3.5 4B, `sh_changelog` (2026-11-01, steps 15-22): blocked from
/// `git reset --hard`, it tried again in other forms (with another target,
/// inside `||`, then `git clean`). Every form is still caught, nothing it
/// tried to destroy is gone, and each denial says to do without it.
#[tokio::test]
async fn qwen_retrying_blocked_commands_in_other_forms() {
    let fx = Fx::new();
    fx.repo();
    let head = fx.git(&["rev-parse", "HEAD"]);
    let mut replies = recorded("qwen-blocked-retries");
    replies.push(text("done"));
    let (out, a, _s) = fx.run(replies, "Write changelog.sh").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    let r = results(&a);
    let denied: Vec<&String> = r
        .iter()
        .filter(|(t, e)| *e && t.contains("is on the always-ask list"))
        .map(|(t, _)| t)
        .collect();
    // Five since `git checkout` asks too (2026-11-04): its plain
    // `git checkout` retry used to run.
    assert_eq!(denied.len(), 5, "{r:#?}");
    assert!(
        denied
            .iter()
            .all(|t| t.contains("Do without it, or say in your answer what you needed."))
    );
    assert!(denied.iter().any(|t| t.contains("`git clean`")));
    assert!(
        denied
            .iter()
            .any(|t| t.starts_with("Not run: `git checkout` is"))
    );
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), head);
    assert!(fx.cwd.join("README.md").exists());
}

/// Qwen 3.5 4B, `feat_logstats_jsonl` (2026-10-31, step 1): a path that
/// repeats the working directory, in the same response as a good call. The
/// bad one is refused with the path to use; the other still runs.
#[tokio::test]
async fn qwen_repeating_the_working_directory_in_a_path() {
    let fx = Fx::new();
    std::fs::create_dir_all(fx.cwd.join("logstats")).unwrap();
    std::fs::write(
        fx.cwd.join("logstats/parse.py"),
        "def parse_clf_line(l):\n    pass\n",
    )
    .unwrap();
    let mut replies = recorded("qwen-doubled-path");
    replies.push(text("ok"));
    let (out, a, _s) = fx.run(replies, "Add JSON lines").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    let r = results(&a);
    let want = format!(
        "repeats the working directory ({}). Use {}/logstats/parse.py instead.",
        fx.cwd.display(),
        fx.cwd.display()
    );
    assert!(r[0].1 && r[0].0.ends_with(&want), "{r:#?}");
    // The other call ran (its absolute /work/project was the trial's).
    assert!(!r[1].1, "{r:#?}");
}

/// Gemma 4 12B, `sh_changelog` (2026-10-31, steps 11-22): the same command
/// again and again. Here the revision doesn't exist, so each call fails the
/// same way: the third gets a nudge, the fifth stops a headless run with
/// the reason, instead of running to the step cap.
#[tokio::test]
async fn gemma_repeating_a_failing_command() {
    let fx = Fx::new();
    fx.repo();
    let mut replies = recorded("gemma-loop");
    replies.push(text("done"));
    let (out, a, s) = fx.run(replies, "Write changelog.sh").await;
    assert_eq!(
        out.stop,
        Stop::Guard("the same call failed 5 times in a row".into()),
        "{:#?}",
        results(&a)
    );
    assert_eq!(s.chat_requests().len(), 5);
    let r = results(&a);
    assert!(
        r[2].0.ends_with("change the call or try another approach."),
        "{r:#?}"
    );
}

/// Gemma 4 12B, `sh_changelog` (2026-10-31, steps 9 and 31): `rm -rf .git`
/// to start the repository over, which checkpoints can't undo. Both now
/// ask (headless: denied), and the history is still there.
#[tokio::test]
async fn gemma_removing_the_repository() {
    let fx = Fx::new();
    fx.repo();
    let head = fx.git(&["rev-parse", "HEAD"]);
    let mut replies = recorded("gemma-rm-git");
    replies.push(text("done"));
    let (out, a, _s) = fx.run(replies, "Write changelog.sh").await;
    assert_eq!(out.stop, Stop::Done, "{:?}", out.stop);
    let r = results(&a);
    assert_eq!(r.len(), 2, "{r:#?}");
    assert!(
        r.iter()
            .all(|(t, e)| *e && t.contains("`rm .git` is on the always-ask list")),
        "{r:#?}"
    );
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), head);
}
