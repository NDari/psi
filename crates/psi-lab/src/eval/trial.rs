//! One trial: an agent works on a task in a container, then the project it
//! leaves is graded.

use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use psi_session::{EntryBody, IdGen, write_entries};
use serde_json::json;

use super::grade::{Grade, grade};
use super::result::{Outcome, TrialResult};
use super::tasks::Task;
use crate::adapter::pi::{PiRun, convert};
use crate::error::{IoContext, other};
use crate::{Result, fsutil};

/// Settings shared by every trial of a run.
#[derive(Debug, Clone)]
pub(crate) struct TrialCtx<'a> {
    pub evals_dir: &'a Path,
    pub run_dir: &'a Path,
    pub port: u16,
    pub n_ctx: u64,
    pub max_output: u64,
    pub steps: u32,
    pub timeout_s: u64,
    pub network: String,
}

/// The step cap extension's stderr line (evals/pi/step-cap.ts).
const STEP_CAP_LINE: &str = "psi eval: step cap reached";

/// Runs trial 0 of `task` with pi as the agent. Anything that goes wrong
/// in the harness becomes a HARNESS-FAIL result rather than an error.
// TODO(phase-2): the same with psi as the agent.
pub(crate) fn run_pi(ctx: &TrialCtx, task: &Task) -> TrialResult {
    let dir = ctx.run_dir.join(&task.name).join("trial-0");
    let start = Instant::now();
    let mut result = match pi_trial(ctx, task, &dir) {
        Ok(r) => r,
        Err(e) => TrialResult {
            task: task.name.clone(),
            trial: 0,
            agent: "pi".into(),
            outcome: Outcome::HarnessFail,
            reason: e.to_string(),
            checks_passed: 0,
            checks_total: task.total,
            steps: 0,
            tool_calls: 0,
            tool_errors: 0,
            tokens_in: 0,
            tokens_out: 0,
            step_cap_hit: false,
            exit_code: -1,
            wall_s: 0,
        },
    };
    result.wall_s = start.elapsed().as_secs();
    let json = serde_json::to_string_pretty(&result).unwrap_or_default();
    if let Err(e) = fsutil::write(&dir.join("result.json"), json + "\n") {
        tracing::warn!("{e}");
    }
    result
}

fn pi_trial(ctx: &TrialCtx, task: &Task, dir: &Path) -> Result<TrialResult> {
    let home = dir.join("home");
    let work = dir.join("work");
    let project = work.join("project");
    fsutil::create_dir_all(&home.join(".pi/agent"))?;
    fsutil::copy_dir(&task.dir.join("repo"), &project, &[])?;
    let models = json!({"providers": {"llamacpp": {
        "baseUrl": format!("http://127.0.0.1:{}/v1", ctx.port),
        "api": "openai-completions",
        "apiKey": "none",
        "models": [{"id": "local", "contextWindow": ctx.n_ctx,
                    "maxTokens": ctx.max_output, "reasoning": false}],
    }}});
    fsutil::write(
        &home.join(".pi/agent/models.json"),
        serde_json::to_string_pretty(&models).unwrap_or_default(),
    )?;
    // The project is /work/project inside the git repository /work.
    git_init(&work)?;

    let prompt = task.prompt()?;
    let timeout = ctx.timeout_s.to_string();
    let exit_code = super::podman::Container::new(&ctx.network, &home)
        .mount(&work, "/work", false)
        .mount(&ctx.evals_dir.join("pi"), "/eval-pi", true)
        .workdir("/work/project")
        .env("PI_OFFLINE", "1")
        .env("PI_SKIP_VERSION_CHECK", "1")
        .env("PSI_EVAL_STEPS", &ctx.steps.to_string())
        .cmd([
            "timeout",
            "-k",
            "30",
            "-s",
            "INT",
            &timeout,
            "pi",
            "--mode",
            "json",
            "-p",
            &prompt,
            "--model",
            "llamacpp/local",
            "--tools",
            "read,bash,edit,write",
            "-e",
            "/eval-pi/step-cap.ts",
        ])
        .run(&dir.join("run.jsonl"), &dir.join("run.stderr"))?;

    let stderr = fsutil::read_to_string(&dir.join("run.stderr"))?;
    let step_cap_hit = stderr.contains(STEP_CAP_LINE);
    let events = std::fs::File::open(dir.join("run.jsonl"))
        .ctx(|| format!("opening {}", dir.join("run.jsonl").display()))?;
    let mut pi = convert(BufReader::new(events))?;
    let ts = pi.entries.last().map(|e| e.ts).unwrap_or(0);
    let end = IdGen::resume(&pi.entries).push(
        ts,
        EntryBody::Custom {
            custom_type: "eval_end".into(),
            data: json!({"exit_code": exit_code, "step_cap_hit": step_cap_hit,
                         "settled": pi.settled, "bad_lines": pi.bad_lines}),
        },
    );
    pi.entries.push(end);
    let session = dir.join("session.jsonl");
    let file = std::fs::File::create(&session).ctx(|| format!("creating {}", session.display()))?;
    write_entries(file, &pi.entries).map_err(|e| other(format!("{}: {e}", session.display())))?;

    let graded = grade(
        task,
        &ctx.evals_dir.join("code/_lib"),
        &project,
        &dir.join("grade"),
    );
    let (outcome, reason) = classify(exit_code, ctx.timeout_s, &pi, step_cap_hit, &graded);
    let g = graded.unwrap_or(Grade {
        passed: 0,
        total: task.total,
    });

    let mut r = TrialResult {
        task: task.name.clone(),
        trial: 0,
        agent: "pi".into(),
        outcome,
        reason,
        checks_passed: g.passed,
        checks_total: g.total,
        steps: 0,
        tool_calls: 0,
        tool_errors: 0,
        tokens_in: 0,
        tokens_out: 0,
        step_cap_hit,
        exit_code,
        wall_s: 0,
    };
    for e in &pi.entries {
        match &e.body {
            EntryBody::Message { usage, .. } if e.is_model_step() => {
                r.steps += 1;
                r.tool_calls += e.tool_calls().count() as u32;
                if let Some(u) = usage {
                    r.tokens_in += u.prompt_total();
                    r.tokens_out += u.output;
                }
            }
            EntryBody::ToolResult { is_error: true, .. } => r.tool_errors += 1,
            _ => {}
        }
    }
    Ok(r)
}

/// Decides the outcome class. Timeouts and the step cap are the model's;
/// a crash, a failed request or a missing final event is the harness's.
fn classify(
    exit_code: i32,
    timeout_s: u64,
    pi: &PiRun,
    step_cap_hit: bool,
    graded: &Result<Grade>,
) -> (Outcome, String) {
    let harness = |r: String| (Outcome::HarnessFail, r);
    if exit_code == 124 {
        return (Outcome::ModelFail, format!("timeout after {timeout_s}s"));
    }
    if !(0..125).contains(&exit_code) {
        return harness(format!(
            "the container failed (exit {exit_code}, see run.stderr)"
        ));
    }
    if exit_code != 0 {
        return harness(format!("pi exited {exit_code} (see run.stderr)"));
    }
    if !pi.settled {
        return harness("pi gave no agent_settled event".into());
    }
    if !step_cap_hit && let Some(e) = &pi.last_error {
        return harness(format!("a model request failed: {e}"));
    }
    let g = match graded {
        Ok(g) => *g,
        Err(e) => return harness(format!("grading failed: {e}")),
    };
    if g.all_passed() {
        return (Outcome::Pass, String::new());
    }
    let mut reason = format!("checks: {} passed of {}", g.passed, g.total);
    if step_cap_hit {
        reason.insert_str(0, "step cap; ");
    }
    (Outcome::ModelFail, reason)
}

fn git_init(work: &Path) -> Result<()> {
    let steps: [&[&str]; 3] = [
        &["init", "-q", "-b", "main"],
        &["add", "-A"],
        &["commit", "-q", "--allow-empty", "-m", "eval: initial"],
    ];
    for args in steps {
        let status = Command::new("git")
            .args([
                "-c",
                "user.name=psi-eval",
                "-c",
                "user.email=eval@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-C",
            ])
            .arg(work)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .ctx(|| "running git".into())?;
        if !status.success() {
            return Err(other(format!(
                "git {} failed in {}",
                args[0],
                work.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled() -> PiRun {
        PiRun {
            settled: true,
            ..PiRun::default()
        }
    }

    fn ok(p: u32, t: u32) -> Result<Grade> {
        Ok(Grade {
            passed: p,
            total: t,
        })
    }

    #[test]
    fn all_checks_pass() {
        let (o, r) = classify(0, 60, &settled(), false, &ok(3, 3));
        assert_eq!((o, r.as_str()), (Outcome::Pass, ""));
    }

    #[test]
    fn some_checks_fail_is_the_models() {
        let (o, r) = classify(0, 60, &settled(), true, &ok(1, 3));
        assert_eq!(o, Outcome::ModelFail);
        assert_eq!(r, "step cap; checks: 1 passed of 3");
    }

    #[test]
    fn timeout_is_the_models() {
        let (o, _) = classify(124, 60, &PiRun::default(), false, &ok(3, 3));
        assert_eq!(o, Outcome::ModelFail);
    }

    #[test]
    fn crashes_and_missing_events_are_the_harness() {
        assert_eq!(
            classify(125, 60, &settled(), false, &ok(3, 3)).0,
            Outcome::HarnessFail
        );
        assert_eq!(
            classify(-1, 60, &settled(), false, &ok(3, 3)).0,
            Outcome::HarnessFail
        );
        assert_eq!(
            classify(1, 60, &settled(), false, &ok(3, 3)).0,
            Outcome::HarnessFail
        );
        let (o, r) = classify(0, 60, &PiRun::default(), false, &ok(3, 3));
        assert_eq!(o, Outcome::HarnessFail);
        assert!(r.contains("agent_settled"));
    }

    #[test]
    fn request_error_is_the_harness_unless_the_step_cap_caused_it() {
        let pi = PiRun {
            settled: true,
            last_error: Some("500".into()),
            ..PiRun::default()
        };
        assert_eq!(
            classify(0, 60, &pi, false, &ok(0, 3)).0,
            Outcome::HarnessFail
        );
        assert_eq!(classify(0, 60, &pi, true, &ok(0, 3)).0, Outcome::ModelFail);
    }

    #[test]
    fn grading_error_is_the_harness() {
        let g = Err(other("podman missing"));
        let (o, r) = classify(0, 60, &settled(), false, &g);
        assert_eq!(o, Outcome::HarnessFail);
        assert_eq!(r, "grading failed: podman missing");
    }
}
