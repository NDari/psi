//! One trial: an agent works on a task in a container, then the project it
//! leaves is graded.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use psi_session::{Entry, EntryBody, IdGen, read_entries, write_entries};
use serde_json::{Value, json};

use super::grade::{Grade, grade};
use super::podman::Container;
use super::result::{Outcome, TrialResult};
use super::run::Agent;
use super::tasks::Task;
use crate::adapter::pi::convert;
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
    /// The psi binary mounted into psi trials.
    pub psi_binary: Option<&'a Path>,
}

/// The step cap extension's stderr line (evals/pi/step-cap.ts).
const STEP_CAP_LINE: &str = "psi eval: step cap reached";

/// Runs trial 0 of `task` with `agent`. Anything that goes wrong in the
/// harness becomes a HARNESS-FAIL result rather than an error.
pub(crate) fn run_trial(ctx: &TrialCtx, task: &Task, agent: Agent) -> TrialResult {
    let dir = ctx.run_dir.join(&task.name).join("trial-0");
    let start = Instant::now();
    let mut result = match trial(ctx, task, agent, &dir) {
        Ok(r) => r,
        Err(e) => TrialResult {
            task: task.name.clone(),
            trial: 0,
            agent: agent.as_str().into(),
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

/// How an agent's run ended, as far as the outcome class cares.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunFacts {
    pub exit_code: i32,
    /// The agent's final event was seen (pi: agent_settled; psi: run_end).
    pub settled: bool,
    /// A model request or the agent itself failed (not the step cap).
    pub failure: Option<String>,
    pub step_cap_hit: bool,
}

fn trial(ctx: &TrialCtx, task: &Task, agent: Agent, dir: &Path) -> Result<TrialResult> {
    let home = dir.join("home");
    let work = dir.join("work");
    let project = work.join("project");
    fsutil::create_dir_all(&home)?;
    fsutil::copy_dir(&task.dir.join("repo"), &project, &[])?;
    // The project is /work/project inside the git repository /work.
    git_init(&work)?;
    let prompt = task.prompt()?;

    let (facts, mut entries) = match agent {
        Agent::Pi => pi_run(ctx, &home, &work, dir, &prompt)?,
        Agent::Psi => psi_run(ctx, &home, &work, dir, &prompt)?,
    };
    let ts = entries.last().map(|e| e.ts).unwrap_or(0);
    let end = IdGen::resume(&entries).push(
        ts,
        EntryBody::Custom {
            custom_type: "eval_end".into(),
            data: json!({"exit_code": facts.exit_code, "step_cap_hit": facts.step_cap_hit,
                         "settled": facts.settled, "failure": facts.failure}),
        },
    );
    entries.push(end);
    let session = dir.join("session.jsonl");
    let file = std::fs::File::create(&session).ctx(|| format!("creating {}", session.display()))?;
    write_entries(file, &entries).map_err(|e| other(format!("{}: {e}", session.display())))?;

    let graded = grade(
        task,
        &ctx.evals_dir.join("code/_lib"),
        &project,
        &dir.join("grade"),
    );
    let (outcome, reason) = classify(agent, &facts, ctx.timeout_s, &graded);
    let g = graded.unwrap_or(Grade {
        passed: 0,
        total: task.total,
    });
    let mut r = TrialResult {
        task: task.name.clone(),
        trial: 0,
        agent: agent.as_str().into(),
        outcome,
        reason,
        checks_passed: g.passed,
        checks_total: g.total,
        steps: 0,
        tool_calls: 0,
        tool_errors: 0,
        tokens_in: 0,
        tokens_out: 0,
        step_cap_hit: facts.step_cap_hit,
        exit_code: facts.exit_code,
        wall_s: 0,
    };
    for e in &entries {
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

fn pi_run(
    ctx: &TrialCtx,
    home: &Path,
    work: &Path,
    dir: &Path,
    prompt: &str,
) -> Result<(RunFacts, Vec<Entry>)> {
    fsutil::create_dir_all(&home.join(".pi/agent"))?;
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
    let timeout = ctx.timeout_s.to_string();
    let exit_code = Container::new(&ctx.network, home)
        .mount(work, "/work", false)
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
            prompt,
            "--model",
            "llamacpp/local",
            "--tools",
            "read,bash,edit,write",
            "-e",
            "/eval-pi/step-cap.ts",
        ])
        .run(&dir.join("run.jsonl"), &dir.join("run.stderr"))?;
    let stderr = fsutil::read_to_string(&dir.join("run.stderr"))?;
    let events = std::fs::File::open(dir.join("run.jsonl"))
        .ctx(|| format!("opening {}", dir.join("run.jsonl").display()))?;
    let pi = convert(BufReader::new(events))?;
    let step_cap_hit = stderr.contains(STEP_CAP_LINE);
    let facts = RunFacts {
        exit_code,
        settled: pi.settled,
        // The step cap's abort shows up as a failed request; it is not one.
        failure: if step_cap_hit {
            None
        } else {
            pi.last_error
                .clone()
                .map(|e| format!("a model request failed: {e}"))
        },
        step_cap_hit,
    };
    Ok((facts, pi.entries))
}

fn psi_run(
    ctx: &TrialCtx,
    home: &Path,
    work: &Path,
    dir: &Path,
    prompt: &str,
) -> Result<(RunFacts, Vec<Entry>)> {
    let binary = ctx
        .psi_binary
        .ok_or_else(|| other("no psi binary to run"))?;
    fsutil::create_dir_all(&home.join(".psi"))?;
    let models = json!({"models": [{"id": "local",
        "base_url": format!("http://127.0.0.1:{}/v1", ctx.port),
        "context_window": ctx.n_ctx, "max_output": ctx.max_output}]});
    fsutil::write(
        &home.join(".psi/models.jsonc"),
        serde_json::to_string_pretty(&models).unwrap_or_default(),
    )?;
    let timeout = ctx.timeout_s.to_string();
    let steps = ctx.steps.to_string();
    let exit_code = Container::new(&ctx.network, home)
        .mount(work, "/work", false)
        .mount(binary, "/usr/local/bin/psi", true)
        .workdir("/work/project")
        .cmd([
            "timeout",
            "-k",
            "30",
            "-s",
            "INT",
            &timeout,
            "psi",
            "-p",
            prompt,
            "--json",
            "--max-turns",
            &steps,
        ])
        .run(&dir.join("run.jsonl"), &dir.join("run.stderr"))?;
    // The run's last event says how it ended.
    let events = fsutil::read_to_string(&dir.join("run.jsonl"))?;
    let end: Option<Value> = events
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .rfind(|v| v["type"] == "run_end");
    let stop = end.as_ref().and_then(|e| e["stop"].as_str()).unwrap_or("");
    let reason = end
        .as_ref()
        .and_then(|e| e["reason"].as_str())
        .unwrap_or("")
        .to_owned();
    let facts = RunFacts {
        exit_code,
        settled: end.is_some(),
        failure: (stop == "failed").then(|| format!("psi failed: {reason}")),
        step_cap_hit: stop == "budget" && reason.contains("model requests"),
    };
    // psi's own session file is the record (D-31); copy it and its wire log.
    let sessions = home.join(".psi/sessions");
    let found = std::fs::read_dir(&sessions)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|d| {
            std::fs::read_dir(d.path())
                .ok()
                .into_iter()
                .flatten()
                .flatten()
        })
        .map(|e| e.path())
        .find(|p| {
            p.extension().is_some_and(|x| x == "jsonl")
                && !p.to_string_lossy().ends_with(".wire.jsonl")
        });
    let entries = match &found {
        Some(p) => {
            let wire = PathBuf::from(format!(
                "{}.wire.jsonl",
                p.to_string_lossy().trim_end_matches(".jsonl")
            ));
            if wire.exists() {
                std::fs::copy(&wire, dir.join("wire.jsonl"))
                    .ctx(|| "copying the wire log".into())?;
            }
            let f = std::fs::File::open(p).ctx(|| format!("opening {}", p.display()))?;
            read_entries(BufReader::new(f)).map_err(|e| other(format!("{}: {e}", p.display())))?
        }
        None => Vec::new(),
    };
    Ok((facts, entries))
}

/// Decides the outcome class. Timeouts and the step cap are the model's;
/// a crash, a failed request or a missing final event is the harness's.
fn classify(
    agent: Agent,
    f: &RunFacts,
    timeout_s: u64,
    graded: &Result<Grade>,
) -> (Outcome, String) {
    let harness = |r: String| (Outcome::HarnessFail, r);
    let name = agent.as_str();
    if f.exit_code == 124 {
        return (Outcome::ModelFail, format!("timeout after {timeout_s}s"));
    }
    if !(0..125).contains(&f.exit_code) {
        return harness(format!(
            "the container failed (exit {}, see run.stderr)",
            f.exit_code
        ));
    }
    if !f.settled {
        return harness(format!(
            "{name} gave no final event (exit {}, see run.stderr)",
            f.exit_code
        ));
    }
    if let Some(e) = &f.failure {
        return harness(e.clone());
    }
    // psi exits 3 when a budget stops the run; anything else non-zero is a failure.
    let budget_exit = agent == Agent::Psi && f.exit_code == 3;
    if f.exit_code != 0 && !budget_exit {
        return harness(format!("{name} exited {} (see run.stderr)", f.exit_code));
    }
    let g = match graded {
        Ok(g) => *g,
        Err(e) => return harness(format!("grading failed: {e}")),
    };
    if g.all_passed() {
        return (Outcome::Pass, String::new());
    }
    let mut reason = format!("checks: {} passed of {}", g.passed, g.total);
    if f.step_cap_hit {
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

    fn facts(exit_code: i32) -> RunFacts {
        RunFacts {
            exit_code,
            settled: true,
            failure: None,
            step_cap_hit: false,
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
        let (o, r) = classify(Agent::Pi, &facts(0), 60, &ok(3, 3));
        assert_eq!((o, r.as_str()), (Outcome::Pass, ""));
    }

    #[test]
    fn some_checks_fail_is_the_models() {
        let f = RunFacts {
            step_cap_hit: true,
            ..facts(0)
        };
        let (o, r) = classify(Agent::Pi, &f, 60, &ok(1, 3));
        assert_eq!(o, Outcome::ModelFail);
        assert_eq!(r, "step cap; checks: 1 passed of 3");
    }

    #[test]
    fn timeout_is_the_models() {
        let f = RunFacts {
            settled: false,
            ..facts(124)
        };
        assert_eq!(
            classify(Agent::Psi, &f, 60, &ok(3, 3)).0,
            Outcome::ModelFail
        );
    }

    #[test]
    fn crashes_and_missing_events_are_the_harness() {
        for agent in [Agent::Pi, Agent::Psi] {
            assert_eq!(
                classify(agent, &facts(125), 60, &ok(3, 3)).0,
                Outcome::HarnessFail
            );
            assert_eq!(
                classify(agent, &facts(-1), 60, &ok(3, 3)).0,
                Outcome::HarnessFail
            );
            assert_eq!(
                classify(agent, &facts(1), 60, &ok(3, 3)).0,
                Outcome::HarnessFail
            );
            let f = RunFacts {
                settled: false,
                ..facts(0)
            };
            let (o, r) = classify(agent, &f, 60, &ok(3, 3));
            assert_eq!(o, Outcome::HarnessFail);
            assert!(r.contains("no final event"), "{r}");
        }
    }

    #[test]
    fn psi_budget_exit_is_the_models() {
        let f = RunFacts {
            step_cap_hit: true,
            ..facts(3)
        };
        assert_eq!(
            classify(Agent::Psi, &f, 60, &ok(0, 3)).0,
            Outcome::ModelFail
        );
        assert_eq!(
            classify(Agent::Pi, &facts(3), 60, &ok(0, 3)).0,
            Outcome::HarnessFail
        );
    }

    #[test]
    fn failures_and_grading_errors_are_the_harness() {
        let f = RunFacts {
            failure: Some("a model request failed: 500".into()),
            ..facts(0)
        };
        assert_eq!(
            classify(Agent::Pi, &f, 60, &ok(0, 3)).0,
            Outcome::HarnessFail
        );
        let (o, r) = classify(Agent::Pi, &facts(0), 60, &Err(other("podman missing")));
        assert_eq!(o, Outcome::HarnessFail);
        assert_eq!(r, "grading failed: podman missing");
    }
}
