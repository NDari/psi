use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use psi_cli::config::{self, SettingsPaths};
use psi_lab::eval::{self, Agent, RunConfig, Selection};
use psi_lab::inspect;
use serde_json::{Map, Value};
use tracing_subscriber::EnvFilter;

/// A coding agent harness for local models.
#[derive(Debug, Parser)]
#[command(name = "psi", version)]
struct Cli {
    /// Safety mode; overrides the settings files (D-11).
    #[arg(long, value_enum)]
    mode: Option<Mode>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mode {
    Auto,
    Strict,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Strict => "strict",
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run coding tasks in podman and grade them (manual; needs llama-server).
    #[command(subcommand)]
    Eval(EvalCommand),
    /// Show a session or eval trial as a timeline, with patterns flagged.
    Inspect(InspectArgs),
}

#[derive(Debug, Subcommand)]
enum EvalCommand {
    /// Check the graders: every reference passes, every untouched repo fails.
    Check {
        #[command(flatten)]
        common: EvalCommon,
        /// quick, all, or task names separated by commas.
        #[arg(long, default_value = "all")]
        tasks: String,
    },
    /// One trial of each task with an agent. A single run proves the
    /// pipeline works; it is not a measure of quality or speed. Exits
    /// non-zero if any trial is a harness failure.
    Run {
        #[command(flatten)]
        common: EvalCommon,
        /// quick, all, or task names separated by commas.
        #[arg(long, default_value = "quick")]
        tasks: String,
        #[arg(long, value_enum, default_value = "pi")]
        agent: AgentArg,
        /// llama-server's port on 127.0.0.1.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Model requests per trial before it is stopped.
        #[arg(long, default_value_t = 40)]
        steps: u32,
        /// Seconds per trial.
        #[arg(long, default_value_t = 1200)]
        timeout: u64,
        /// The agent's output token limit per response.
        #[arg(long, default_value_t = 8192)]
        max_output: u64,
        /// Appended to the results directory's name.
        #[arg(long)]
        label: Option<String>,
    },
    /// Two runs side by side, pointing to the trials that changed.
    Diff { a: PathBuf, b: PathBuf },
}

#[derive(Debug, Args)]
struct EvalCommon {
    /// The evals directory (Containerfile, code/, pi/).
    #[arg(long, default_value = "evals")]
    evals: PathBuf,
    /// Where results go (default: ~/.psi/evals/<UTC time>[-label]).
    #[arg(long)]
    results: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AgentArg {
    Pi,
}

#[derive(Debug, Args)]
struct InspectArgs {
    /// A session file, or a trial directory holding session.jsonl.
    path: PathBuf,
    /// Print JSON instead of the terminal view.
    #[arg(long)]
    json: bool,
    /// Only steps with a failed tool call, and failed requests.
    #[arg(long)]
    failures: bool,
    /// Only guard events.
    #[arg(long)]
    guards: bool,
    /// Only blocked or asked policy decisions.
    #[arg(long)]
    blocked: bool,
    /// Only step N, in full.
    #[arg(long)]
    step: Option<u32>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("psi: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Logs go to stderr, filtered by `PSI_LOG` (for example `PSI_LOG=debug`).
fn init_tracing() {
    let filter = EnvFilter::try_from_env("PSI_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Some(Command::Eval(cmd)) => eval_command(cmd),
        Some(Command::Inspect(args)) => inspect_command(args),
        None => {
            let cwd = std::env::current_dir().context("reading the working directory")?;
            let paths = SettingsPaths::discover(std::env::home_dir().as_deref(), &cwd);
            let settings = config::load(&paths, flags_layer(&cli))?;
            tracing::debug!(%settings, "settings loaded");
            // TODO(phase-2): print and JSON modes. TODO(phase-4): interactive mode.
            eprintln!("psi: no modes are built yet; see `psi --help`");
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn flags_layer(cli: &Cli) -> Value {
    let mut m = Map::new();
    if let Some(mode) = cli.mode {
        m.insert("mode".into(), mode.as_str().into());
    }
    Value::Object(m)
}

fn progress(line: &str) {
    eprintln!("eval: {line}");
}

fn results_dir(given: Option<PathBuf>, suffix: Option<&str>) -> Result<PathBuf> {
    if let Some(p) = given {
        return Ok(p);
    }
    let home = std::env::home_dir().context("no home directory; pass --results")?;
    let mut name = psi_lab::utc_stamp(psi_lab::now_ms());
    if let Some(s) = suffix {
        name = format!("{name}-{s}");
    }
    Ok(home.join(".psi/evals").join(name))
}

fn check_evals_dir(dir: &Path) -> Result<()> {
    if !dir.join("Containerfile").is_file() || !dir.join("code").is_dir() {
        bail!(
            "{} is not the evals directory; run from the psi repository or pass --evals",
            dir.display()
        );
    }
    Ok(())
}

fn eval_command(cmd: EvalCommand) -> Result<ExitCode> {
    match cmd {
        EvalCommand::Check { common, tasks } => {
            check_evals_dir(&common.evals)?;
            let results = results_dir(common.results, Some("check"))?;
            let report = eval::check_graders(
                &common.evals,
                &results,
                &Selection::parse(&tasks),
                &mut progress,
            )?;
            let good = report.rows.iter().filter(|r| r.ok()).count();
            println!(
                "graders: {good} of {} right (the reference passes every check, the untouched repo does not); outputs in {}",
                report.rows.len(),
                results.display()
            );
            Ok(if report.ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        EvalCommand::Run {
            common,
            tasks,
            agent,
            port,
            steps,
            timeout,
            max_output,
            label,
        } => {
            check_evals_dir(&common.evals)?;
            let agent = match agent {
                AgentArg::Pi => Agent::Pi,
            };
            let suffix = label.unwrap_or_else(|| agent.as_str().to_owned());
            let cfg = RunConfig {
                evals_dir: common.evals,
                results_dir: results_dir(common.results, Some(&suffix))?,
                agent,
                selection: Selection::parse(&tasks),
                port,
                steps,
                timeout_s: timeout,
                max_output,
            };
            let results = eval::run(&cfg, &mut progress)?;
            let summary = std::fs::read_to_string(cfg.results_dir.join("summary.md"))
                .context("reading summary.md")?;
            println!("{summary}");
            println!("results in {}", cfg.results_dir.display());
            // The run is an end-to-end test of the harness: it fails only on
            // harness failures, never on the model's.
            let bad = eval::harness_failures(&results);
            if bad.is_empty() {
                return Ok(ExitCode::SUCCESS);
            }
            let names: Vec<&str> = bad.iter().map(|r| r.task.as_str()).collect();
            eprintln!(
                "psi: {} harness failure(s): {}",
                bad.len(),
                names.join(", ")
            );
            Ok(ExitCode::FAILURE)
        }
        EvalCommand::Diff { a, b } => {
            println!("{}", eval::diff_runs(&a, &b)?);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn inspect_command(args: InspectArgs) -> Result<ExitCode> {
    let record = inspect::load(&args.path)?;
    let filter = inspect::Filter {
        failures: args.failures,
        guards: args.guards,
        blocked: args.blocked,
        step: args.step,
    };
    let t = filter.apply(&inspect::timeline(&record));
    if args.json {
        println!("{}", serde_json::to_string_pretty(&t)?);
    } else {
        print!("{}", inspect::render_text(&t, args.step.is_some()));
    }
    Ok(ExitCode::SUCCESS)
}
