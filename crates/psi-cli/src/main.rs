mod complete;
mod interactive;
mod run_mode;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCandidates;
use psi_cli::config::{self, SettingsPaths};
use psi_lab::eval::{self, Agent, RunConfig, Selection};
use psi_lab::inspect;
use serde_json::{Map, Value};
use tracing_subscriber::EnvFilter;

/// A coding agent harness for local models.
#[derive(Debug, Parser)]
#[command(name = "psi", version, disable_help_subcommand = true)]
struct Cli {
    /// Run one prompt and print the answer ("-" reads the prompt from stdin).
    #[arg(short = 'p', long = "print", value_name = "PROMPT")]
    print: Option<String>,

    /// With -p: print the run's events as JSON lines instead of the answer.
    #[arg(long, requires = "print")]
    json: bool,

    /// The model profile from ~/.psi/models.jsonc (default: the first, or llama-server on 9931).
    #[arg(long, add = ArgValueCandidates::new(complete::models))]
    model: Option<String>,

    /// Continue the latest session in this directory.
    #[arg(short = 'c', long = "continue", conflicts_with = "session")]
    continue_last: bool,

    /// Continue this session file.
    #[arg(long, value_name = "FILE", add = ArgValueCandidates::new(complete::sessions))]
    session: Option<PathBuf>,

    /// Model requests per run: print runs stop there (default 40); the TUI pauses and asks (default: no limit).
    #[arg(long, value_name = "N")]
    max_turns: Option<u32>,

    /// Safety mode; overrides the settings files (D-11).
    #[arg(long, value_enum)]
    mode: Option<Mode>,

    /// Allow an always-ask entry without asking, like git-push (headless runs deny the rest).
    #[arg(long, value_name = "ENTRY")]
    allow: Vec<String>,

    /// Run bash inside the sandbox (bubblewrap on Linux, Seatbelt on macOS), or directly when something else is the boundary.
    #[arg(long, value_enum)]
    isolation: Option<IsolationArg>,

    /// Let bash reach the network, or cut it off (needs isolation).
    #[arg(long, value_enum)]
    network: Option<OnOff>,

    /// Load an extension (a .lua file, a directory with init.lua, or a script tool directory).
    #[arg(long = "extension", value_name = "PATH")]
    extensions: Vec<PathBuf>,

    /// Set an extension's flag.
    #[arg(long = "ext-flag", value_name = "NAME=VALUE")]
    ext_flags: Vec<String>,

    /// Trust this project's .psi settings and extensions without asking (headless runs skip them otherwise).
    #[arg(long)]
    trust_project: bool,

    /// Start in plan mode: read-only exploration that ends with a numbered plan.
    #[arg(long)]
    plan: bool,

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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IsolationArg {
    // "bwrap" was the only value before macOS had a sandbox.
    #[value(alias = "bwrap")]
    On,
    Off,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run coding tasks in podman and grade them (manual; needs llama-server).
    #[command(subcommand)]
    Eval(EvalCommand),
    /// Show a session or eval trial as a timeline, with patterns flagged.
    Inspect(InspectArgs),
    /// Resend the exact request of one step from a session's wire log.
    Replay(ReplayArgs),
    /// Serve a recorded run's model responses like llama-server (for tests).
    #[command(hide = true)]
    ReplayServer(ReplayServerArgs),
    /// Extension author tools: lint and test an extension.
    #[command(subcommand)]
    Ext(ExtCommand),
    /// MCP servers from ~/.psi/mcp.json and .psi/mcp.json.
    #[command(subcommand)]
    Mcp(McpCommand),
    /// Add a package (a local directory, or git:host/user/repo[@ref]) with its extensions, skills, prompts and themes.
    Install {
        source: String,
        /// In this project's .psi/settings.jsonc instead of ~/.psi.
        #[arg(short = 'l', long)]
        local: bool,
    },
    /// Remove a package; a git clone is deleted.
    #[command(alias = "uninstall")]
    Remove {
        source: String,
        #[arg(short = 'l', long)]
        local: bool,
    },
    /// Fetch git packages again: pinned ones to their ref, the rest to their branch.
    Update { source: Option<String> },
    /// List the installed packages.
    List,
    /// Short answers about psi's features, for you and the model.
    Help {
        #[arg(add = ArgValueCandidates::new(complete::topics))]
        topic: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum McpCommand {
    /// Connect to every enabled server and show its state, tools and their
    /// size in tokens; exits 1 when one failed or an entry is invalid.
    List,
}

#[derive(Debug, Subcommand)]
enum ExtCommand {
    /// Lint an extension: manifest, syntax, API use, a dry load, and
    /// lua-language-server when installed.
    Check { path: PathBuf },
    /// Run an extension's *_test.lua files (a directory of extensions runs them all).
    Test {
        path: PathBuf,
        /// Show what passing tests printed too.
        #[arg(long)]
        verbose: bool,
    },
    /// List the example extensions, or print one with its tests.
    Example {
        #[arg(add = ArgValueCandidates::new(complete::examples))]
        name: Option<String>,
    },
}

#[derive(Debug, Args)]
struct ReplayArgs {
    /// A session file, its .wire.jsonl, or an eval trial directory.
    path: PathBuf,
    /// The step (model request) to resend, from 1.
    #[arg(long)]
    at: usize,
    /// How many continuations to draw (each with its own seed).
    #[arg(long, default_value_t = 1)]
    samples: u32,
    /// The model profile to send to (default: the first in models.jsonc).
    #[arg(long)]
    model: Option<String>,
    // TODO(phase-7): --variant (§12.4).
}

#[derive(Debug, Args)]
struct ReplayServerArgs {
    /// A session file whose assistant messages are played back in order.
    #[arg(long, conflicts_with = "script")]
    session: Option<PathBuf>,
    /// A JSON list of replies (psi_ai::replay::Reply).
    #[arg(long)]
    script: Option<PathBuf>,
    #[arg(long, default_value_t = 9931)]
    port: u16,
}

#[derive(Debug, Subcommand)]
enum EvalCommand {
    /// Check the graders: every reference passes, every untouched repo fails.
    Check {
        #[command(flatten)]
        common: EvalCommon,
        /// quick, all, or task names separated by commas.
        #[arg(long, default_value = "all", add = ArgValueCandidates::new(complete::tasks))]
        tasks: String,
        /// The psi binary graders may run (default: this one). It must run
        /// in the Debian image: `just eval-build` makes a static one.
        #[arg(long, value_name = "PATH")]
        psi_binary: Option<PathBuf>,
    },
    /// One trial of each task with an agent. A single run proves the
    /// pipeline works; it is not a measure of quality or speed. Exits
    /// non-zero if any trial is a harness failure.
    Run {
        #[command(flatten)]
        common: EvalCommon,
        /// quick, all, or task names separated by commas.
        #[arg(long, default_value = "quick", add = ArgValueCandidates::new(complete::tasks))]
        tasks: String,
        #[arg(long, value_enum, default_value = "psi")]
        agent: AgentArg,
        /// llama-server's port on 127.0.0.1.
        #[arg(long, default_value_t = 9931)]
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
        /// The context window the agent is told (default: the server's). Smaller
        /// than the server's, it tests compaction without restarting llama-server.
        #[arg(long, value_name = "TOKENS")]
        n_ctx: Option<u64>,
        /// Appended to the results directory's name.
        #[arg(long)]
        label: Option<String>,
        /// The psi binary the trials run (default: this one). It must run in
        /// the Debian image: `just eval-build` makes a static one.
        #[arg(long, value_name = "PATH")]
        psi_binary: Option<PathBuf>,
        /// A directory copied into each psi trial's ~/.psi (settings.jsonc,
        /// skills/, prompts/): a variant, like a battery turned on.
        #[arg(long, value_name = "DIR")]
        psi_config: Option<PathBuf>,
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
    Psi,
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
    // `COMPLETE=fish psi` prints the fish script; the shell then calls psi
    // with COMPLETE set to get candidates, and this answers and exits.
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();
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
        Some(Command::Replay(args)) => runtime()?.block_on(replay_command(args)),
        Some(Command::ReplayServer(args)) => replay_server_command(args),
        Some(Command::Ext(cmd)) => ext_command(cmd),
        Some(Command::Mcp(McpCommand::List)) => {
            let cwd = std::env::current_dir().context("reading the working directory")?;
            let home = std::env::home_dir();
            let trusted = project_trusted(home.as_deref(), &cwd, false, false)?;
            let (text, ok) =
                runtime()?.block_on(psi_cli::mcp::list(home.as_deref(), &cwd, trusted));
            println!("{text}");
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Some(
            cmd @ (Command::Install { .. }
            | Command::Remove { .. }
            | Command::Update { .. }
            | Command::List),
        ) => package_command(cmd),
        Some(Command::Help { topic }) => help_command(topic.as_deref()),
        None => {
            let cwd = std::env::current_dir().context("reading the working directory")?;
            let home = std::env::home_dir();
            let interactive = cli.print.is_none();
            let trusted = project_trusted(home.as_deref(), &cwd, interactive, cli.trust_project)?;
            let mut paths = SettingsPaths::discover(home.as_deref(), &cwd);
            if !trusted {
                paths.project = None;
            }
            let settings = config::load(&paths, flags_layer(&cli))?;
            tracing::debug!(%settings, "settings loaded");
            let args = run_mode::RunArgs {
                prompt: cli.print.unwrap_or_default(),
                json: cli.json,
                model: cli.model,
                continue_last: cli.continue_last,
                session: cli.session,
                max_turns: cli.max_turns,
                settings,
                allow: cli.allow,
                mode_flag: cli.mode.is_some(),
                extensions: cli.extensions,
                ext_flags: cli.ext_flags,
                project_trusted: trusted,
                plan: cli.plan,
            };
            if interactive {
                if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
                    bail!(
                        "the interactive mode needs a terminal; run a prompt with psi -p \"...\""
                    );
                }
                return interactive::run(args);
            }
            runtime()?.block_on(run_mode::run(args))
        }
    }
}

/// Whether this project's own .psi may load (§9.3): trusted before with the
/// same files, or the user says yes now. Headless runs don't ask.
fn project_trusted(home: Option<&Path>, cwd: &Path, interactive: bool, flag: bool) -> Result<bool> {
    let Some(print) = psi_cli::trust::snapshot(cwd) else {
        return Ok(true);
    };
    let Some(home) = home else { return Ok(false) };
    if psi_cli::trust::is_trusted(home, cwd, &print) {
        return Ok(true);
    }
    let what = psi_cli::trust::describe(cwd);
    if flag {
        psi_cli::trust::trust(home, cwd, &print)?;
        return Ok(true);
    }
    if !interactive || !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        eprintln!(
            "psi: {what}; not loading them until you trust them (run psi here once, or pass --trust-project)"
        );
        return Ok(false);
    }
    eprintln!("psi: {what}.");
    for c in psi_cli::trust::changes(home, cwd, &print) {
        eprintln!("  {c}");
    }
    eprint!("Load it? [y/N] ");
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("reading the answer")?;
    if matches!(answer.trim(), "y" | "Y" | "yes") {
        psi_cli::trust::trust(home, cwd, &print)?;
        return Ok(true);
    }
    eprintln!("psi: not loading {}", cwd.join(".psi").display());
    Ok(false)
}

fn ext_command(cmd: ExtCommand) -> Result<ExitCode> {
    match cmd {
        ExtCommand::Example { name: None } => {
            print!("{}", psi_cli::examples::list());
            Ok(ExitCode::SUCCESS)
        }
        ExtCommand::Example { name: Some(name) } => match psi_cli::examples::show(&name) {
            Some(text) => {
                print!("{text}");
                Ok(ExitCode::SUCCESS)
            }
            None => bail!("no example named {name}\n{}", psi_cli::examples::list()),
        },
        ExtCommand::Check { path } => {
            let report = psi_ext::check::check(&path);
            print!("{}", report.render());
            Ok(if report.errors() == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        ExtCommand::Test { path, verbose } => {
            let runs = if path.is_dir() && !path.join("init.lua").is_file() {
                psi_ext::testing::run_all(&path)
            } else {
                vec![(path.clone(), psi_ext::testing::run(&path))]
            };
            if runs.is_empty() {
                bail!("{}: no extensions with *_test.lua files", path.display());
            }
            let mut ok = true;
            for (p, r) in &runs {
                if runs.len() > 1 {
                    println!("== {}", p.display());
                }
                print!("{}", r.render(verbose));
                ok &= r.passed();
            }
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
    }
}

fn package_command(cmd: Command) -> Result<ExitCode> {
    use psi_cli::packages::{self, Paths, Scope};
    let paths = Paths {
        home: std::env::home_dir().context("no home directory")?,
        cwd: std::env::current_dir().context("reading the working directory")?,
    };
    let scope = |local: bool| if local { Scope::Project } else { Scope::User };
    match cmd {
        Command::Install { source, local } => {
            println!("{}", packages::install(&paths, scope(local), &source)?)
        }
        Command::Remove { source, local } => {
            println!("{}", packages::remove(&paths, scope(local), &source)?)
        }
        Command::Update { source } => {
            let done = packages::update(&paths, source.as_deref())?;
            if done.is_empty() {
                println!("no git packages to update");
            }
            for d in done {
                println!("{d}");
            }
        }
        _ => {
            let mut any = false;
            for (scope, title) in [(Scope::User, "user"), (Scope::Project, "project")] {
                let list = packages::listed(&paths, scope)?;
                if list.is_empty() {
                    continue;
                }
                any = true;
                println!("{title} packages ({}):", paths.settings(scope).display());
                for p in list {
                    println!("  {p}");
                }
            }
            if !any {
                println!("no packages installed");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn help_command(topic: Option<&str>) -> Result<ExitCode> {
    use psi_cli::help;
    match topic {
        None | Some("topics") => print!("{}", help::topics()),
        Some(t) => match help::topic(t) {
            Some(text) => print!("{text}"),
            None => {
                print!("no topic '{t}'. {}", help::topics());
                return Ok(ExitCode::FAILURE);
            }
        },
    }
    Ok(ExitCode::SUCCESS)
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")
}

/// The wire log for a session file, a wire log, or a trial directory.
fn wire_path(p: &Path) -> PathBuf {
    if p.is_dir() {
        return p.join("wire.jsonl");
    }
    let s = p.to_string_lossy();
    if s.ends_with(".wire.jsonl") {
        return p.to_owned();
    }
    PathBuf::from(format!("{}.wire.jsonl", s.trim_end_matches(".jsonl")))
}

async fn replay_command(args: ReplayArgs) -> Result<ExitCode> {
    let wire = wire_path(&args.path);
    let body = psi_lab::replay::request_at(&wire, args.at)?;
    let profile =
        run_mode::pick_profile(std::env::home_dir().as_deref(), args.model.as_deref(), &[])?;
    let client = psi_ai::Client::new(&profile.base_url).with_headers(&profile.headers);
    let (actions, dist) = psi_lab::replay::replay(&client, body, args.samples).await?;
    if args.samples <= 1 {
        println!("{}", actions.join("\n"));
    } else {
        for (action, n) in dist.iter().rev() {
            println!("{n:>3}  {action}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn replay_server_command(args: ReplayServerArgs) -> Result<ExitCode> {
    use psi_ai::replay::{ReplayServer, Reply};
    let replies: Vec<Reply> = match (&args.session, &args.script) {
        (Some(s), _) => {
            let f = std::fs::File::open(s).with_context(|| format!("opening {}", s.display()))?;
            let entries = psi_session::read_entries(std::io::BufReader::new(f))?;
            psi_lab::replay::script_from_entries(&entries)
        }
        (None, Some(p)) => {
            let text =
                std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
            serde_json::from_str(&text)
                .with_context(|| format!("{}: not a list of replies", p.display()))?
        }
        (None, None) => bail!("give --session or --script"),
    };
    let n = replies.len();
    let server = ReplayServer::start_on(args.port, replies)?;
    eprintln!("replay server: {n} replies on {}", server.base_url());
    loop {
        std::thread::park();
    }
}

fn flags_layer(cli: &Cli) -> Value {
    let mut m = Map::new();
    if let Some(mode) = cli.mode {
        m.insert("mode".into(), mode.as_str().into());
    }
    if let Some(i) = cli.isolation {
        let v = match i {
            IsolationArg::On => "on",
            IsolationArg::Off => "off",
        };
        m.insert("isolation".into(), v.into());
    }
    if let Some(n) = cli.network {
        let v = match n {
            OnOff::On => "on",
            OnOff::Off => "off",
        };
        m.insert("network".into(), v.into());
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
        EvalCommand::Check {
            common,
            tasks,
            psi_binary,
        } => {
            check_evals_dir(&common.evals)?;
            let results = results_dir(common.results, Some("check"))?;
            let psi = psi_binary
                .map(|p| std::path::absolute(&p).unwrap_or(p))
                .or_else(|| std::env::current_exe().ok());
            let report = eval::check_graders(
                &common.evals,
                &results,
                psi.as_deref(),
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
            n_ctx,
            label,
            psi_binary,
            psi_config,
        } => {
            check_evals_dir(&common.evals)?;
            let agent = match agent {
                AgentArg::Psi => Agent::Psi,
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
                n_ctx,
                psi_binary: psi_binary
                    .map(|p| std::path::absolute(&p).unwrap_or(p))
                    .or_else(|| std::env::current_exe().ok()),
                psi_config: psi_config.map(|p| std::path::absolute(&p).unwrap_or(p)),
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
