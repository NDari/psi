//! Print and JSON modes: one prompt, run headless (D-18).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use psi_agent::approval::{Approver, Headless};
use psi_agent::prompt::{PromptFacts, project_context, system_prompt};
use psi_agent::{Agent, AgentConfig, Budgets, Stop};
use psi_ai::profile::{ContextWindow, ModelProfile};
use psi_ai::{AiError, Client, WireLog};
use psi_cli::config;
use psi_cli::safety::{SafetySetup, SetupPaths, setup_with};
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
    /// What loading the settings dropped, said once at startup.
    pub notes: Vec<String>,
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

/// Everything a mode needs to run the agent.
pub struct Prepared {
    pub agent: Agent,
    pub profile: ModelProfile,
    pub n_ctx: u64,
    /// The model server wasn't up (the TUI only): connect when it is.
    pub offline: bool,
    pub tmpdir: PathBuf,
    pub cwd: PathBuf,
    pub home: Option<PathBuf>,
    /// `~/.psi/sessions`.
    pub sessions: PathBuf,
    /// Said once at startup.
    pub warnings: Vec<String>,
    /// The safety setup, as recorded in `session_info`.
    pub info: Value,
}

/// The model check, the session, safety and the agent, for print, JSON
/// and interactive modes alike.
pub async fn prepare(args: &RunArgs, approver: Arc<dyn Approver>) -> Result<Prepared> {
    let home = std::env::home_dir();
    let cwd = std::env::current_dir().context("reading the working directory")?;
    let mode = if args.prompt.is_empty() && !args.json {
        "interactive"
    } else if args.json {
        "json"
    } else {
        "print"
    };
    let profile = pick_profile(home.as_deref(), args.model.as_deref())?;
    let client = Client::new(&profile.base_url).with_headers(&profile.headers);

    // D-09: the template must support tool calls; take the window from /props.
    let props = client.props().await;
    // The TUI starts without a server and connects when it comes up; the
    // other modes need it now.
    let offline = not_up(&props) && mode == "interactive";
    let (n_ctx, server, progress) = match props {
        Err(e @ AiError::Connect { .. }) if !offline => {
            bail!("{e}. Is llama-server running? (just llm-up)")
        }
        _ if offline => (profile_window(&profile, None), None, false),
        props => server_facts(&profile, props, mode == "interactive")?,
    };
    let client = client.with_progress(progress);

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
    let unreadable = skipped_note(&session);
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
    } = setup_with(
        &args.settings,
        &SetupPaths {
            cwd: &cwd,
            home: home.as_deref(),
            tmpdir: &tmpdir,
            session: &stem,
        },
        approver,
    )?;
    let mut warnings = warnings;
    warnings.extend(args.notes.iter().cloned());
    warnings.extend(unreadable);
    let tools = core_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    let facts = PromptFacts {
        checkpoints: safety.checkpoints.is_some(),
    };
    let system = system_prompt(&names, &cwd, &project_context(&cwd, home.as_deref()), facts);
    let mut ctx = ToolCtx::new(&cwd, &tmpdir, result_chars(n_ctx));
    ctx.sandbox = sandbox;
    let mut budgets = Budgets::default();
    match args.max_turns {
        Some(n) => budgets.max_turns = n,
        // The TUI has no cap on requests or tool calls unless one is asked
        // for (pi parity): Esc stops a run and the guards still pause it.
        // Print runs keep the default as a backstop against a runaway loop.
        None if mode == "interactive" => {
            budgets.max_turns = u32::MAX;
            budgets.max_tool_calls = u32::MAX;
        }
        None => {}
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
    let info = safety.info.clone();
    let post_edit = psi_agent::PostEditCheck::from_setting(&args.settings["post_edit_check"])
        .map_err(anyhow::Error::msg)?;
    let compaction = psi_agent::CompactionSettings::from_setting(&args.settings["compaction"])
        .map_err(anyhow::Error::msg)?;
    let agent = Agent::new(client, session, cfg, ctx)
        .with_safety(safety)
        .with_post_edit_check(post_edit)
        .with_server(server)
        .with_compaction(compaction);
    Ok(Prepared {
        agent,
        profile,
        n_ctx,
        offline,
        tmpdir,
        cwd,
        home,
        sessions,
        warnings,
        info,
    })
}

/// The window: the profile's, or the server's, or a fallback.
fn profile_window(profile: &ModelProfile, server: Option<u64>) -> u64 {
    match profile.context_window {
        ContextWindow::Tokens(n) => n,
        ContextWindow::Auto => server.unwrap_or(FALLBACK_CONTEXT),
    }
}

/// The cap on one tool result, in characters, for a window.
pub fn result_chars(n_ctx: u64) -> usize {
    (n_ctx as f64 * RESULT_SHARE * CHARS_PER_TOKEN) as usize
}

/// The server isn't up yet: nothing listens, or llama-server is still
/// loading the model (503), when `/props` has no window to give.
pub fn not_up(props: &Result<psi_ai::Props, AiError>) -> bool {
    matches!(
        props,
        Err(AiError::Connect { .. } | AiError::Http { status: 503, .. })
    )
}

/// What `/props` tells: the window, the server's record for the session,
/// and whether to ask for prompt progress (llama-server in the TUI; other
/// servers may refuse the field). Fails when the template can't call tools.
pub fn server_facts(
    profile: &ModelProfile,
    props: Result<psi_ai::Props, AiError>,
    interactive: bool,
) -> Result<(u64, Option<Value>, bool)> {
    match props {
        Ok(p) => {
            if p.supports_tool_calls == Some(false) {
                bail!("{}", AiError::NoToolSupport);
            }
            let file = p
                .model_path
                .as_deref()
                .and_then(|m| Path::new(m).file_name())
                .map(|f| f.to_string_lossy().into_owned());
            // For the session record: requests name only the alias.
            let server = serde_json::json!({"model_file": file, "model_path": p.model_path,
                                            "n_ctx": p.n_ctx, "build": p.build_info});
            Ok((profile_window(profile, p.n_ctx), Some(server), interactive))
        }
        Err(e) => {
            tracing::warn!("could not read /props: {e}");
            Ok((profile_window(profile, None), None, false))
        }
    }
}

/// A warning when opening the session skipped lines it couldn't read.
pub fn skipped_note(s: &SessionFile) -> Option<String> {
    let n = s.skipped().len();
    (n > 0).then(|| {
        format!(
            "{}: {n} line(s) couldn't be read (a crash while saving?) and were left out; the rest of the session is here",
            s.path().display()
        )
    })
}

pub async fn run(args: RunArgs) -> Result<ExitCode> {
    let Prepared {
        mut agent,
        tmpdir,
        warnings,
        ..
    } = prepare(&args, Arc::new(Headless)).await?;
    for w in &warnings {
        eprintln!("psi: {w}");
    }

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
        // Notices go to stderr like warnings.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        agent = agent.with_events(tx);
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                match ev {
                    psi_agent::AgentEvent::Notice { text } => eprintln!("psi: {text}"),
                    psi_agent::AgentEvent::Retrying { reason } => {
                        eprintln!("psi: {reason}; trying again");
                    }
                    _ => {}
                }
            }
        });
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
        if s.trim().is_empty() {
            bail!("the prompt from stdin was empty");
        }
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
