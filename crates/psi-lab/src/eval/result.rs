//! What a run and each trial record.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The three outcome classes (§12.5). Harness failures never count as
/// model results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    #[serde(rename = "PASS")]
    Pass,
    #[serde(rename = "MODEL-FAIL")]
    ModelFail,
    #[serde(rename = "HARNESS-FAIL")]
    HarnessFail,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Pass => "PASS",
            Outcome::ModelFail => "MODEL-FAIL",
            Outcome::HarnessFail => "HARNESS-FAIL",
        }
    }
}

/// `<run>/<task>/trial-<i>/result.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrialResult {
    pub task: String,
    pub trial: u32,
    pub agent: String,
    pub outcome: Outcome,
    /// Why it is not a PASS, or empty.
    pub reason: String,
    pub checks_passed: u32,
    pub checks_total: u32,
    /// Model responses with content (a step is one request plus its tool calls).
    pub steps: u32,
    pub tool_calls: u32,
    pub tool_errors: u32,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub step_cap_hit: bool,
    /// The agent's exit code, or -1 when it did not run.
    pub exit_code: i32,
    pub wall_s: u64,
}

impl TrialResult {
    pub fn score(&self) -> f64 {
        if self.checks_total == 0 {
            0.0
        } else {
            f64::from(self.checks_passed) / f64::from(self.checks_total)
        }
    }
}

/// A run passes, as an end-to-end test of the harness, when no trial is a
/// HARNESS-FAIL. Model failures are correct results.
pub fn harness_failures(results: &[TrialResult]) -> Vec<&TrialResult> {
    results
        .iter()
        .filter(|r| r.outcome == Outcome::HarnessFail)
        .collect()
}

/// `<run>/run.json`: what the run used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMeta {
    pub agent: String,
    pub psi_version: String,
    pub started_ms: u64,
    pub tasks: Vec<String>,
    pub steps: u32,
    pub timeout_s: u64,
    pub max_output: u64,
    pub n_ctx: u64,
    /// llama-server's model path, from `/props`.
    pub model: String,
    /// llama-server's build, from `/props`.
    pub server_build: String,
    /// `/props` as the server reported it at the start of the run.
    pub server_props: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(task: &str, outcome: Outcome) -> TrialResult {
        TrialResult {
            task: task.into(),
            trial: 0,
            agent: "pi".into(),
            outcome,
            reason: String::new(),
            checks_passed: 0,
            checks_total: 1,
            steps: 0,
            tool_calls: 0,
            tool_errors: 0,
            tokens_in: 0,
            tokens_out: 0,
            step_cap_hit: false,
            exit_code: 0,
            wall_s: 0,
        }
    }

    #[test]
    fn only_harness_failures_fail_a_run() {
        let rs = [r("a", Outcome::Pass), r("b", Outcome::ModelFail)];
        assert!(harness_failures(&rs).is_empty());
        let rs = [r("a", Outcome::ModelFail), r("b", Outcome::HarnessFail)];
        let bad: Vec<&str> = harness_failures(&rs)
            .iter()
            .map(|r| r.task.as_str())
            .collect();
        assert_eq!(bad, ["b"]);
    }
}
