//! Run summaries and `psi eval diff`.

use std::fmt::Write;
use std::path::Path;

use super::result::{Outcome, RunMeta, TrialResult};
use crate::error::other;
use crate::{Result, fsutil};

/// Reads every `<run>/<task>/trial-<i>/result.json`, sorted by task and trial.
pub fn load_results(run_dir: &Path) -> Result<Vec<TrialResult>> {
    let mut out = Vec::new();
    let tasks = std::fs::read_dir(run_dir)
        .map_err(|e| other(format!("reading {}: {e}", run_dir.display())))?;
    for task in tasks.flatten() {
        let Ok(trials) = std::fs::read_dir(task.path()) else {
            continue;
        };
        for trial in trials.flatten() {
            let path = trial.path().join("result.json");
            if !path.is_file() {
                continue;
            }
            let text = fsutil::read_to_string(&path)?;
            let r: TrialResult = serde_json::from_str(&text)
                .map_err(|e| other(format!("{}: {e}", path.display())))?;
            out.push(r);
        }
    }
    if out.is_empty() {
        return Err(other(format!(
            "no trial results under {}",
            run_dir.display()
        )));
    }
    out.sort_by(|a, b| (&a.task, a.trial).cmp(&(&b.task, b.trial)));
    Ok(out)
}

pub fn load_meta(run_dir: &Path) -> Option<RunMeta> {
    let text = fsutil::read_to_string(&run_dir.join("run.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// `summary.md`. Wall time is recorded per trial but left out here: a run
/// shows the pipeline works, it does not measure speed.
pub fn summary_markdown(meta: Option<&RunMeta>, results: &[TrialResult]) -> String {
    let mut s = String::from("# Eval run\n\n");
    if let Some(m) = meta {
        let server_ctx = m.server_props["default_generation_settings"]["n_ctx"].as_u64();
        let ctx = match server_ctx {
            Some(n) if n != m.n_ctx => format!("{} (the server's: {n})", m.n_ctx),
            _ => m.n_ctx.to_string(),
        };
        let _ = writeln!(
            s,
            "Agent {}. Model `{}`, llama-server build {}, context {ctx}, step cap {}, max output {}.\n",
            m.agent, m.model, m.server_build, m.steps, m.max_output
        );
    }
    s.push_str(
        "A single run proves the pipeline works end to end. It is not a measure of quality or speed.\n\n",
    );
    s.push_str(
        "| Task | Outcome | Checks | Steps | Tool calls | Tool errors | Tokens in/out | Reason |\n",
    );
    s.push_str("|---|---|---|---|---|---|---|---|\n");
    for r in results {
        let _ = writeln!(
            s,
            "| {} | {} | {}/{} | {} | {} | {} | {}/{} | {} |",
            r.task,
            r.outcome.as_str(),
            r.checks_passed,
            r.checks_total,
            r.steps,
            r.tool_calls,
            r.tool_errors,
            r.tokens_in,
            r.tokens_out,
            r.reason.replace('|', "\\|")
        );
    }
    let count = |o: Outcome| results.iter().filter(|r| r.outcome == o).count();
    let _ = writeln!(
        s,
        "\nOutcomes: {} PASS, {} MODEL-FAIL, {} HARNESS-FAIL.",
        count(Outcome::Pass),
        count(Outcome::ModelFail),
        count(Outcome::HarnessFail)
    );
    s
}

/// Two runs side by side, pointing to the trials whose outcome or checks
/// changed.
pub fn diff_runs(a_dir: &Path, b_dir: &Path) -> Result<String> {
    let a = load_results(a_dir)?;
    let b = load_results(b_dir)?;
    Ok(diff_results(a_dir, &a, b_dir, &b))
}

fn diff_results(a_dir: &Path, a: &[TrialResult], b_dir: &Path, b: &[TrialResult]) -> String {
    let key = |r: &TrialResult| (r.task.clone(), r.trial);
    let mut keys: Vec<(String, u32)> = a.iter().chain(b).map(key).collect();
    keys.sort();
    keys.dedup();
    let find = |rs: &[TrialResult], k: &(String, u32)| rs.iter().find(|r| key(r) == *k).cloned();
    let cell = |r: &Option<TrialResult>| match r {
        Some(r) => format!(
            "{} {}/{}",
            r.outcome.as_str(),
            r.checks_passed,
            r.checks_total
        ),
        None => "-".into(),
    };
    let mut s = format!("A: {}\nB: {}\n\n", a_dir.display(), b_dir.display());
    s.push_str("| Task | Trial | A | B | |\n|---|---|---|---|---|\n");
    let mut changed = Vec::new();
    for k in &keys {
        let (ra, rb) = (find(a, k), find(b, k));
        let differs = match (&ra, &rb) {
            (Some(x), Some(y)) => x.outcome != y.outcome || x.checks_passed != y.checks_passed,
            _ => true,
        };
        if differs {
            changed.push(k.clone());
        }
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} |",
            k.0,
            k.1,
            cell(&ra),
            cell(&rb),
            if differs { "changed" } else { "" }
        );
    }
    if changed.is_empty() {
        s.push_str("\nNo trial changed.\n");
    } else {
        s.push_str("\nChanged trials (inspect both):\n");
        for (t, i) in changed {
            let _ = writeln!(
                s,
                "- {t}: {}  {}",
                a_dir.join(&t).join(format!("trial-{i}")).display(),
                b_dir.join(&t).join(format!("trial-{i}")).display()
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(task: &str, outcome: Outcome, passed: u32) -> TrialResult {
        TrialResult {
            task: task.into(),
            trial: 0,
            agent: "pi".into(),
            outcome,
            reason: String::new(),
            checks_passed: passed,
            checks_total: 3,
            steps: 5,
            tool_calls: 4,
            tool_errors: 1,
            tokens_in: 100,
            tokens_out: 10,
            step_cap_hit: false,
            exit_code: 0,
            wall_s: 9,
        }
    }

    #[test]
    fn summary_counts_outcomes_and_leaves_out_time() {
        let s = summary_markdown(
            None,
            &[r("a", Outcome::Pass, 3), r("b", Outcome::HarnessFail, 0)],
        );
        assert!(
            s.contains("| a | PASS | 3/3 | 5 | 4 | 1 | 100/10 |  |"),
            "{s}"
        );
        assert!(s.contains("1 PASS, 0 MODEL-FAIL, 1 HARNESS-FAIL"), "{s}");
        assert!(s.contains("not a measure of quality or speed"));
        assert!(!s.contains("wall"));
    }

    #[test]
    fn diff_marks_changed_and_missing_trials() {
        let a = [r("a", Outcome::Pass, 3), r("b", Outcome::ModelFail, 1)];
        let b = [r("a", Outcome::Pass, 3), r("c", Outcome::ModelFail, 2)];
        let s = diff_results(Path::new("/A"), &a, Path::new("/B"), &b);
        assert!(s.contains("| a | 0 | PASS 3/3 | PASS 3/3 |  |"), "{s}");
        assert!(
            s.contains("| b | 0 | MODEL-FAIL 1/3 | - | changed |"),
            "{s}"
        );
        assert!(s.contains("- c: /A/c/trial-0  /B/c/trial-0"), "{s}");
    }

    #[test]
    fn results_round_trip_through_a_run_dir() {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("a/trial-0");
        std::fs::create_dir_all(&t).unwrap();
        let want = r("a", Outcome::Pass, 3);
        std::fs::write(t.join("result.json"), serde_json::to_string(&want).unwrap()).unwrap();
        std::fs::write(dir.path().join("summary.md"), "x").unwrap();
        assert_eq!(load_results(dir.path()).unwrap(), vec![want]);
    }
}
