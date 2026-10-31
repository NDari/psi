//! Print and JSON modes: one prompt, run headless (D-18).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use psi_agent::prompt::{PromptFacts, project_context, system_prompt};
use psi_agent::{Agent, AgentConfig, Budgets, Stop};
use psi_ai::profile::{ContextWindow, ModelProfile};
use psi_ai::{AiError, Client, WireLog};
use psi_cli::config;
use psi_cli::safety::{SafetySetup, SetupPaths, setup};
use psi_session::SessionFile;
use psi_tools::{ToolCtx, core_tools};
use serde_json::Value;

/// Used when neither models.jsonc nor the server says.
const FALLBACK_CONTEXT: u64 = 32_768;
/// Tool results are capped at this share of the window (§6).
const RESULT_SHARE: f64 = 0.15;
/// Conservative characters per token (D-20).
const CHARS_PER_TOKEN: f64 = 3.0;

pub struct RunArgs {
    pub prompt: String,
    pub json: bool,
    pub model: Option<String>,
    pub continue_last: bool,
    pub session: Option<PathBuf>,
    pub max_turns: Option<u32>,
    /// The merged settings layers.
    pub settings: Value,
    /// `--allow` entries.
    pub allow: Vec<String>,
    /// The mode was given on the command line.
    pub mode_flag: bool,
}

/// Exit codes: 0 done, 1 failed, 3 a budget stopped the run, 130 aborted.
pub fn exit_code(stop: &Stop) -> ExitCode {
    ExitCode::from(match stop {
        Stop::Done => 0,
        Stop::Failed(_) => 1,
        Stop::Budget(_) | Stop::Guard(_) => 3,
        Stop::Aborted => 130,
    })
}

pub fn pick_profile(home: Option<&Path>, model: Option<&str>) -> Result<ModelProfile> {
    let models = config::load_models(home)?;
    match model {
        Some(id) => models
            .models
            .into_iter()
            .find(|m| m.id == id)
            .with_context(|| format!("no model '{id}' in ~/.psi/models.jsonc")),
        None => Ok(models
            .models
            .into_iter()
            .next()
            .unwrap_or_else(ModelProfile::local)),
    }
}

pub async fn run(args: RunArgs) -> Result<ExitCode> {
    let home = std::env::home_dir();
    let cwd = std::env::current_dir().context("reading the working directory")?;
    let profile = pick_profile(home.as_deref(), args.model.as_deref())?;
    let client = Client::new(&profile.base_url);

    // D-09: the template must support tool calls; take the window from /props.
    let n_ctx = match client.props().await {
        Ok(p) => {
            if p.supports_tool_calls == Some(false) {
                bail!("{}", AiError::NoToolSupport);
            }
            match profile.context_window {
                ContextWindow::Tokens(n) => n,
                ContextWindow::Auto => p.n_ctx.unwrap_or(FALLBACK_CONTEXT),
            }
        }
        Err(e @ AiError::Connect { .. }) => bail!("{e}. Is llama-server running? (just llm-up)"),
        Err(e) => {
            tracing::warn!("could not read /props: {e}");
            match profile.context_window {
                ContextWindow::Tokens(n) => n,
                ContextWindow::Auto => FALLBACK_CONTEXT,
            }
        }
    };

    let sessions = home
        .as_deref()
        .map(|h| h.join(".psi/sessions"))
        .context("no home directory")?;
    let session = match (&args.session, args.continue_last) {
        (Some(p), _) => SessionFile::open(p).with_context(|| format!("opening {}", p.display()))?,
        (None, true) => match SessionFile::latest(&sessions, &cwd) {
            Some(p) => SessionFile::open(&p).with_context(|| format!("opening {}", p.display()))?,
            None => bail!("no earlier session in {}", cwd.display()),
        },
        (None, false) => {
            SessionFile::create(&sessions, &cwd).context("creating the session file")?
        }
    };
    let wire = WireLog::open(&session.wire_path()).context("opening the wire log")?;
    let client = client.with_wire(Arc::new(wire));
    let stem = session
        .path()
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmpdir = home
        .as_deref()
        .map(|h| h.join(".cache/psi/tmp").join(&stem))
        .context("no home directory")?;

    let SafetySetup {
        safety,
        sandbox,
        warnings,
    } = setup(
        &args.settings,
        &args.allow,
        args.mode_flag,
        &SetupPaths {
            cwd: &cwd,
            home: home.as_deref(),
            tmpdir: &tmpdir,
            session: &stem,
        },
    )?;
    for w in &warnings {
        eprintln!("psi: {w}");
    }
    let tools = core_tools(profile.tools.grep_find_ls);
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    let facts = PromptFacts {
        checkpoints: safety.checkpoints.is_some(),
        sandboxed: sandbox.is_some(),
    };
    let system = system_prompt(&names, &cwd, &project_context(&cwd, home.as_deref()), facts);
    let max_chars = (n_ctx as f64 * RESULT_SHARE * CHARS_PER_TOKEN) as usize;
    let mut ctx = ToolCtx::new(&cwd, &tmpdir, max_chars);
    ctx.sandbox = sandbox;
    let mut budgets = Budgets::default();
    if let Some(n) = args.max_turns {
        budgets.max_turns = n;
    }
    let cfg = AgentConfig {
        model: profile.id.clone(),
        max_output: profile.max_output,
        n_ctx,
        enable_thinking: profile.reasoning.enable_thinking,
        budgets,
        system_prompt: system,
        tools,
        home: home.clone(),
    };
    let mut agent = Agent::new(client, session, cfg, ctx).with_safety(safety);

    let printer = if args.json {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        agent = agent.with_events(tx);
        Some(tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                if let Ok(line) = serde_json::to_string(&ev) {
                    let mut out = std::io::stdout().lock();
                    let _ = writeln!(out, "{line}");
                    let _ = out.flush();
                }
            }
        }))
    } else {
        None
    };
    let cancel = agent.cancel_token();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            cancel.cancel();
        }
    });

    let prompt = if args.prompt == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .context("reading the prompt from stdin")?;
        s
    } else {
        args.prompt.clone()
    };
    let out = agent.run(&prompt).await;
    let session_path = agent.session().path().to_owned();
    drop(agent);
    if let Some(p) = printer {
        let _ = p.await;
    }
    let _ = std::fs::remove_dir_all(&tmpdir);

    if !args.json {
        println!("{}", out.text.trim_end());
    }
    match &out.stop {
        Stop::Done => {}
        Stop::Budget(r) | Stop::Guard(r) => eprintln!("psi: stopped: {r}"),
        Stop::Failed(r) => eprintln!("psi: failed: {r}"),
        Stop::Aborted => eprintln!("psi: aborted"),
    }
    tracing::info!(session = %session_path.display(), "run finished");
    Ok(exit_code(&out.stop))
}
