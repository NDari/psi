//! A whole eval run, and the grader check.

use std::net::TcpListener;
use std::path::{Path, PathBuf};

use super::grade::{Grade, grade};
use super::podman::{Container, build_image};
use super::result::{RunMeta, TrialResult};
use super::server::probe;
use super::summary::summary_markdown;
use super::tasks::{Selection, load_tasks};
use super::trial::{TrialCtx, run_trial};
use crate::error::other;
use crate::{Result, fsutil};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Pi,
    Psi,
}

impl Agent {
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::Pi => "pi",
            Agent::Psi => "psi",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    /// The `evals/` directory: `Containerfile`, `code/`, `pi/`.
    pub evals_dir: PathBuf,
    /// Where this run's records go; must not hold an earlier run.
    pub results_dir: PathBuf,
    pub agent: Agent,
    pub selection: Selection,
    /// llama-server's port on 127.0.0.1.
    pub port: u16,
    pub steps: u32,
    pub timeout_s: u64,
    pub max_output: u64,
    /// The psi binary for psi trials (the one running `psi eval`).
    pub psi_binary: Option<PathBuf>,
    /// Copied into each psi trial's `~/.psi` (settings, skills, prompts):
    /// a variant, such as a battery turned on.
    pub psi_config: Option<PathBuf>,
}

/// Runs one trial of each selected task and writes `run.json`, the trial
/// records and `summary.md`. A single run proves the pipeline works; it is
/// not a measure of quality or speed.
pub fn run(cfg: &RunConfig, progress: &mut dyn FnMut(&str)) -> Result<Vec<TrialResult>> {
    let tasks = load_tasks(&cfg.evals_dir.join("code"), &cfg.selection)?;
    let server = probe(cfg.port)?;
    if cfg.results_dir.join("run.json").exists() {
        return Err(other(format!(
            "{} already holds a run; pick a new results directory",
            cfg.results_dir.display()
        )));
    }
    fsutil::create_dir_all(&cfg.results_dir)?;
    progress(&format!("results in {}", cfg.results_dir.display()));
    build_image(&cfg.evals_dir, &cfg.results_dir.join("build.log"))?;

    let network = format!("pasta:--splice-only,-T,{}", cfg.port);
    let net = network_check(&network, cfg.port, &cfg.results_dir)?;
    progress(&format!("network: {}", net.join("; ")));
    if cfg.agent == Agent::Psi {
        binary_check(cfg.psi_binary.as_deref(), &cfg.results_dir)?;
    }

    let meta = RunMeta {
        agent: cfg.agent.as_str().into(),
        psi_version: env!("CARGO_PKG_VERSION").into(),
        started_ms: psi_session::time::now_ms(),
        tasks: tasks.iter().map(|t| t.name.clone()).collect(),
        steps: cfg.steps,
        timeout_s: cfg.timeout_s,
        max_output: cfg.max_output,
        n_ctx: server.n_ctx,
        model: server.props["model_path"]
            .as_str()
            .unwrap_or("?")
            .to_owned(),
        server_build: server.props["build_info"]
            .as_str()
            .unwrap_or("?")
            .to_owned(),
        server_props: server.props.clone(),
        psi_config: cfg.psi_config.as_ref().map(|p| p.display().to_string()),
    };
    fsutil::write(
        &cfg.results_dir.join("run.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default() + "\n",
    )?;

    let ctx = TrialCtx {
        evals_dir: &cfg.evals_dir,
        run_dir: &cfg.results_dir,
        port: cfg.port,
        n_ctx: server.n_ctx,
        max_output: cfg.max_output,
        steps: cfg.steps,
        timeout_s: cfg.timeout_s,
        network,
        psi_binary: cfg.psi_binary.as_deref(),
        psi_config: cfg.psi_config.as_deref(),
    };
    let mut results = Vec::new();
    for task in &tasks {
        progress(&format!("{}: running", task.name));
        let r = run_trial(&ctx, task, cfg.agent);
        let mut line = format!(
            "{}: {} (checks {}/{}, {} steps)",
            r.task,
            r.outcome.as_str(),
            r.checks_passed,
            r.checks_total,
            r.steps
        );
        if !r.reason.is_empty() {
            line += &format!("; {}", r.reason);
        }
        progress(&line);
        results.push(r);
    }
    let summary = summary_markdown(Some(&meta), &results);
    fsutil::write(&cfg.results_dir.join("summary.md"), &summary)?;
    Ok(results)
}

/// Checks from a trial container that the model server is the only route
/// out: it must be reachable, and the internet and another host port must
/// not. Returns one line per probe; fails the run otherwise.
/// The psi binary must start in the trial image. One built against the
/// host's glibc may not (the image's is older); the static musl build
/// always does (`just eval-build`).
fn binary_check(binary: Option<&Path>, results: &Path) -> Result<()> {
    let binary = binary.ok_or_else(|| other("no psi binary to run"))?;
    let home = results.join("binary-home");
    fsutil::create_dir_all(&home)?;
    let (out, err) = (results.join("binary.txt"), results.join("binary.stderr"));
    let code = Container::new("none", &home)
        .mount(binary, "/usr/local/bin/psi", true)
        .cmd(["psi", "--version"])
        .run(&out, &err)?;
    if code != 0 {
        let why = fsutil::read_to_string(&err).unwrap_or_default();
        return Err(other(format!(
            "{} does not run in the eval image: {}. Build the static one with `just eval-build` and pass it with --psi-binary",
            binary.display(),
            why.trim()
        )));
    }
    Ok(())
}

fn network_check(network: &str, port: u16, results: &Path) -> Result<Vec<String>> {
    // A host port that is listening but not forwarded must stay unreachable.
    let decoy = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| other(format!("binding a probe port: {e}")))?;
    let decoy_port = decoy
        .local_addr()
        .map_err(|e| other(format!("binding a probe port: {e}")))?
        .port();
    let script = format!(
        r#"probe() {{ if timeout 5 bash -c "exec 3<>/dev/tcp/$1/$2" 2>/dev/null; then echo "$3: reachable"; else echo "$3: blocked"; fi; }}
probe 127.0.0.1 {port} model-server
probe 127.0.0.1 {decoy_port} other-host-port
probe 1.1.1.1 80 internet-ip
probe example.com 443 internet-dns"#
    );
    let home = results.join("network-home");
    fsutil::create_dir_all(&home)?;
    let out = results.join("network.txt");
    Container::new(network, &home)
        .cmd(["bash", "-c", &script])
        .run(&out, &results.join("network.stderr"))?;
    drop(decoy);
    let text = fsutil::read_to_string(&out)?;
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let ok = lines.iter().any(|l| l == "model-server: reachable")
        && lines.len() == 4
        && lines
            .iter()
            .filter(|l| !l.starts_with("model-server"))
            .all(|l| l.ends_with(": blocked"));
    if !ok {
        return Err(other(format!(
            "the network check failed (see {}): {}",
            out.display(),
            lines.join("; ")
        )));
    }
    Ok(lines)
}

/// One task's grader check: the reference must pass every check and the
/// untouched repo must not.
#[derive(Debug, Clone)]
pub struct CheckRow {
    pub task: String,
    pub reference: Grade,
    pub blank: Grade,
}

impl CheckRow {
    pub fn ok(&self) -> bool {
        self.reference.all_passed() && !self.blank.all_passed()
    }
}

#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub rows: Vec<CheckRow>,
}

impl CheckReport {
    pub fn ok(&self) -> bool {
        self.rows.iter().all(CheckRow::ok)
    }
}

/// Grades each task's `repo/` with `reference/` overlaid, and the bare `repo/`.
pub fn check_graders(
    evals_dir: &Path,
    results_dir: &Path,
    selection: &Selection,
    progress: &mut dyn FnMut(&str),
) -> Result<CheckReport> {
    let tasks = load_tasks(&evals_dir.join("code"), selection)?;
    fsutil::create_dir_all(results_dir)?;
    progress(&format!("results in {}", results_dir.display()));
    build_image(evals_dir, &results_dir.join("build.log"))?;
    let lib = evals_dir.join("code/_lib");
    let mut report = CheckReport::default();
    for task in &tasks {
        let out = results_dir.join(&task.name);
        let src = out.join("reference-src");
        fsutil::copy_dir(&task.dir.join("repo"), &src, &[])?;
        fsutil::copy_dir(&task.dir.join("reference"), &src, &[])?;
        let reference = grade(task, &lib, &src, &out.join("reference"))?;
        let blank = grade(task, &lib, &task.dir.join("repo"), &out.join("blank"))?;
        let row = CheckRow {
            task: task.name.clone(),
            reference,
            blank,
        };
        progress(&format!(
            "{:<24} reference {}/{}  blank {}/{}  {}",
            row.task,
            reference.passed,
            reference.total,
            blank.passed,
            blank.total,
            if row.ok() { "ok" } else { "WRONG" }
        ));
        report.rows.push(row);
    }
    Ok(report)
}
