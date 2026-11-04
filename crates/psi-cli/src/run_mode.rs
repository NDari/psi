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
use psi_cli::resources::{Resources, Sources};
use psi_cli::safety::{SafetySetup, SetupPaths, setup_with};
use psi_ext::{ExtManager, Limits, Request as ExtRequest, Shared};
use psi_session::SessionFile;
use psi_tools::{ToolCtx, core_tools};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;

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
    /// `--extension` paths.
    pub extensions: Vec<PathBuf>,
    /// `--ext-flag name=value`.
    pub ext_flags: Vec<String>,
    /// The project's own .psi may load (§9.3).
    pub project_trusted: bool,
    /// Start in plan mode (D-34).
    pub plan: bool,
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

/// `$NAME` reads the environment (pi parity); anything else is literal.
fn env_value(v: &str, what: &str) -> Result<String> {
    match v.strip_prefix('$') {
        Some(name) if !name.is_empty() => {
            std::env::var(name).with_context(|| format!("{what}: ${name} is not set"))
        }
        _ => Ok(v.to_owned()),
    }
}

/// The models extensions registered (`psi.register_provider`), as
/// profiles, each with the name `provider/id`.
fn provider_profiles(providers: &[psi_ext::ProviderInfo]) -> Vec<(String, ModelProfile)> {
    let mut out = Vec::new();
    for p in providers {
        for m in &p.models {
            let mut profile = ModelProfile::local();
            profile.id = m.id.clone();
            profile.base_url = p.base_url.clone();
            if let Some(n) = m.context_window {
                profile.context_window = ContextWindow::Tokens(n);
            }
            if let Some(n) = m.max_output {
                profile.max_output = n;
            }
            profile.reasoning.enable_thinking = m.enable_thinking;
            profile.provider = p.name.clone();
            out.push((format!("{}/{}", p.name, m.id), profile));
        }
    }
    out
}

/// The request headers a registered provider needs, with `$NAME` read now.
fn provider_headers(
    profile: &ModelProfile,
    providers: &[psi_ext::ProviderInfo],
) -> Result<Vec<(String, String)>> {
    let Some(p) = providers.iter().find(|p| p.name == profile.provider) else {
        return Ok(Vec::new());
    };
    let what = format!("provider {}", p.name);
    let mut h = Vec::new();
    if let Some(k) = &p.api_key {
        h.push((
            "Authorization".into(),
            format!("Bearer {}", env_value(k, &what)?),
        ));
    }
    for (k, v) in &p.headers {
        h.push((k.clone(), env_value(v, &what)?));
    }
    Ok(h)
}

pub fn pick_profile(
    home: Option<&Path>,
    model: Option<&str>,
    providers: &[psi_ext::ProviderInfo],
) -> Result<ModelProfile> {
    let models = config::load_models(home)?;
    match model {
        Some(id) => {
            let mut p = match models.models.into_iter().find(|m| m.id == id) {
                Some(p) => p,
                None => {
                    let registered = provider_profiles(providers);
                    registered
                        .iter()
                        .find(|(full, _)| full == id)
                        .or_else(|| registered.iter().find(|(_, p)| p.id == id))
                        .map(|(_, p)| p.clone())
                        .with_context(|| {
                            format!("no model '{id}' in ~/.psi/models.jsonc or from an extension")
                        })?
                }
            };
            p.headers = provider_headers(&p, providers)?;
            Ok(p)
        }
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
    /// What extensions ask of the UI.
    pub ext_requests: UnboundedReceiver<(String, ExtRequest)>,
    /// What `/reload` loads again.
    pub ext_sources: ExtSources,
    /// Skills and prompt templates (D-35).
    pub resources: Arc<Resources>,
}

/// Where extensions come from, for loading and `/reload`.
#[derive(Debug, Clone)]
pub struct ExtSources {
    pub home: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub extra: Vec<PathBuf>,
    /// Packages' extension directories, each holding extensions.
    pub dirs: Vec<PathBuf>,
}

impl ExtSources {
    pub fn find(&self) -> (Vec<psi_ext::Source>, Vec<String>) {
        let (mut found, mut errors) =
            psi_ext::sources(self.home.as_deref(), self.project.as_deref(), &self.extra);
        for d in &self.dirs {
            for r in psi_ext::load::discover(d) {
                match r {
                    Ok(s) => found.push(s),
                    Err(e) => errors.push(e),
                }
            }
        }
        (found, errors)
    }
}

/// Extensions whose psi.exec runs outside the sandbox, from the settings
/// only (never an extension's own manifest, D-45):
/// `"extensions": {"<name>": {"unsandboxed": true}}`.
fn unsandboxed_extensions(settings: &Value) -> Vec<String> {
    settings["extensions"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(_, v)| v["unsandboxed"].as_bool() == Some(true))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Starts the extensions; their load errors become warnings.
async fn start_extensions(
    sources: &ExtSources,
    shared: Shared,
    warnings: &mut Vec<String>,
) -> (Arc<ExtManager>, UnboundedReceiver<(String, ExtRequest)>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (found, errors) = sources.find();
    warnings.extend(errors.into_iter().map(|e| format!("extension: {e}")));
    // Started even with none found, so extensions that /reload loads later
    // can reach the UI.
    let shared = Arc::new(std::sync::RwLock::new(shared));
    let started = tokio::task::spawn_blocking(move || {
        ExtManager::start(&found, shared, Limits::default(), tx)
    })
    .await;
    match started {
        Ok((m, errors)) => {
            warnings.extend(errors.into_iter().map(|e| format!("extension: {e}")));
            (m, rx)
        }
        Err(e) => {
            warnings.push(format!("extensions: {e}"));
            (ExtManager::empty(), rx)
        }
    }
}

/// The model check, the session, safety and the agent, for print, JSON
/// and interactive modes alike.
pub async fn prepare(args: &RunArgs, approver: Arc<dyn Approver>) -> Result<Prepared> {
    let home = std::env::home_dir();
    let cwd = std::env::current_dir().context("reading the working directory")?;
    // Packages (G-04): their resources load after the user's own.
    let mut pkg_warnings = Vec::new();
    let pkgs = match &home {
        Some(h) => {
            let paths = psi_cli::packages::Paths {
                home: h.clone(),
                cwd: cwd.clone(),
            };
            let (c, w) = psi_cli::packages::installed(&paths, args.project_trusted);
            pkg_warnings = w;
            c
        }
        None => Vec::new(),
    };
    // Extensions start first: they may register the model's provider.
    let mut extra = args.extensions.clone();
    extra.extend(pkgs.iter().flat_map(|p| p.extensions.clone()));
    let ext_sources = ExtSources {
        home: home.clone(),
        project: args.project_trusted.then(|| cwd.clone()),
        extra,
        dirs: pkgs.iter().flat_map(|p| p.extension_dirs.clone()).collect(),
    };
    let mut flags = std::collections::BTreeMap::new();
    for f in &args.ext_flags {
        let Some((k, v)) = f.split_once('=') else {
            bail!("--ext-flag {f}: use NAME=VALUE");
        };
        flags.insert(k.to_owned(), v.to_owned());
    }
    let mode = if args.prompt.is_empty() && !args.json {
        "interactive"
    } else if args.json {
        "json"
    } else {
        "print"
    };
    let shared = Shared {
        cwd: cwd.display().to_string(),
        mode: mode.into(),
        has_ui: mode == "interactive",
        flags,
        ..Shared::default()
    };
    let mut ext_warnings = Vec::new();
    let (ext, ext_requests) = start_extensions(&ext_sources, shared, &mut ext_warnings).await;
    ext.set_unsandboxed(unsandboxed_extensions(&args.settings));

    let profile = match pick_profile(home.as_deref(), args.model.as_deref(), &ext.providers()) {
        Ok(p) => p,
        // An extension that failed to load may be the one with the model.
        Err(e) if !ext_warnings.is_empty() => bail!("{e:#}\n{}", ext_warnings.join("\n")),
        Err(e) => return Err(e),
    };
    let client = Client::new(&profile.base_url).with_headers(&profile.headers);

    // D-09: the template must support tool calls; take the window from /props.
    let props = client.props().await;
    // The TUI starts without a server and connects when it comes up; the
    // other modes need it now.
    let offline = matches!(props, Err(AiError::Connect { .. })) && mode == "interactive";
    let (n_ctx, server, progress) = match props {
        Err(e @ AiError::Connect { .. }) if !offline => {
            bail!("{e}. Is llama-server running? (just llm-up)")
        }
        Err(AiError::Connect { .. }) => (profile_window(&profile, None), None, false),
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
        &args.allow,
        args.mode_flag,
        &SetupPaths {
            cwd: &cwd,
            home: home.as_deref(),
            tmpdir: &tmpdir,
            session: &stem,
        },
        approver,
    )?;
    let resources = Arc::new(load_resources(
        &args.settings,
        home.as_deref(),
        &cwd,
        args.project_trusted,
        &pkgs,
    ));
    let mut warnings = warnings;
    warnings.extend(unreadable);
    warnings.extend(pkg_warnings);
    warnings.extend(resources.warnings.iter().cloned());
    let mut tools = core_tools(profile.tools.grep_find_ls);
    if args.settings["todo"].as_bool() == Some(true) {
        tools.push(psi_tools::Tool::Todo);
    }
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    let facts = PromptFacts {
        checkpoints: safety.checkpoints.is_some(),
    };
    let system = system_prompt(
        &names,
        &cwd,
        &project_context(&cwd, home.as_deref()),
        &resources.prompt_skills(),
        facts,
    );
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
    warnings.extend(ext_warnings);
    let post_edit = psi_agent::PostEditCheck::from_setting(&args.settings["post_edit_check"])
        .map_err(anyhow::Error::msg)?;
    let compaction = psi_agent::CompactionSettings::from_setting(&args.settings["compaction"])
        .map_err(anyhow::Error::msg)?;
    let mcp = psi_cli::mcp::start(home.as_deref(), &cwd, args.project_trusted);
    warnings.extend(mcp.problems().iter().map(|p| format!("mcp: {p}")));
    let mut agent = Agent::new(client, session, cfg, ctx)
        .with_safety(safety)
        .with_extensions(ext.clone())
        .with_post_edit_check(post_edit)
        .with_server(server)
        .with_compaction(compaction)
        .with_mcp(mcp);
    if args.settings["subagents"].as_bool() == Some(true) {
        let defs = psi_agent::subagent::with_built_in(resources.agents.clone());
        agent = agent.with_subagents(defs);
    }
    agent.restore_plan();
    if args.plan {
        agent.set_plan_mode(true).map_err(anyhow::Error::msg)?;
    }
    let file = agent.session().path().display().to_string();
    ext.notify("session_start", serde_json::json!({"session_file": file}))
        .await;
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
        ext_requests,
        ext_sources,
        resources,
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

/// Paths from a settings list, with `~` expanded; relative ones are taken
/// from the working directory.
pub fn setting_paths(v: &Value, home: Option<&Path>, cwd: &Path) -> Vec<PathBuf> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|p| match (p.strip_prefix("~/"), home) {
            (Some(rest), Some(h)) => h.join(rest),
            _ => cwd.join(p),
        })
        .collect()
}

/// Skills and prompt templates from the standard places and the
/// `skills` and `prompts` settings.
pub fn load_resources(
    settings: &Value,
    home: Option<&Path>,
    cwd: &Path,
    trusted: bool,
    packages: &[psi_cli::packages::Contents],
) -> Resources {
    let mut skills = setting_paths(&settings["skills"], home, cwd);
    let mut prompts = setting_paths(&settings["prompts"], home, cwd);
    skills.extend(packages.iter().flat_map(|p| p.skills.clone()));
    prompts.extend(packages.iter().flat_map(|p| p.prompts.clone()));
    let mut src = Sources::standard(home, cwd, trusted, &skills, &prompts);
    src.themes
        .extend(packages.iter().flat_map(|p| p.themes.clone()));
    Resources::load(&src)
}

/// Headless runs answer extension UI requests themselves (D-18): nothing
/// prompts, notifications go to stderr, and messages queue for the run.
pub fn answer_headless(mut rx: UnboundedReceiver<(String, ExtRequest)>, queues: psi_agent::Queues) {
    tokio::spawn(async move {
        while let Some((ext, r)) = rx.recv().await {
            match r {
                ExtRequest::Notify { message, level } => {
                    eprintln!("psi: [{ext}] {level}: {message}")
                }
                ExtRequest::Confirm { reply, .. } => {
                    let _ = reply.send(false);
                }
                ExtRequest::Select { reply, .. } | ExtRequest::Input { reply, .. } => {
                    let _ = reply.send(None);
                }
                ExtRequest::Custom { reply, .. } => {
                    let _ = reply.send(None);
                }
                ExtRequest::SendUserMessage { text, follow_up } => {
                    let q = if follow_up {
                        &queues.follow_up
                    } else {
                        &queues.steering
                    };
                    if let Ok(mut q) = q.lock() {
                        q.push_back(text);
                    }
                }
                _ => {}
            }
        }
    });
}

pub async fn run(args: RunArgs) -> Result<ExitCode> {
    let Prepared {
        mut agent,
        tmpdir,
        warnings,
        ext_requests,
        resources,
        ..
    } = prepare(&args, Arc::new(Headless)).await?;
    for w in &warnings {
        eprintln!("psi: {w}");
    }
    answer_headless(ext_requests, agent.queues());

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
        // Notices (an MCP server that failed) go to stderr like warnings.
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
        s
    } else {
        args.prompt.clone()
    };
    // The `input` event, before skill and template expansion (pi parity).
    let mut prompt = prompt;
    let ext = agent.extensions().clone();
    if ext.listens("input") {
        let out = ext
            .chain(
                "input",
                serde_json::json!({"text": prompt.trim(), "source": "print"}),
            )
            .await;
        if out["handled"] == serde_json::Value::Bool(true) {
            let session_path = agent.session().path().to_owned();
            ext.notify(
                "session_shutdown",
                serde_json::json!({"session_file": session_path}),
            )
            .await;
            ext.shutdown();
            drop(agent);
            if let Some(p) = printer {
                let _ = p.await;
            }
            let _ = std::fs::remove_dir_all(&tmpdir);
            return Ok(ExitCode::SUCCESS);
        }
        if let Some(t) = out["text"].as_str() {
            prompt = t.to_owned();
        }
    }
    // `/skill:name` and prompt templates work headless too (pi parity).
    let prompt = match resources.expand(prompt.trim()) {
        Ok(Some(e)) => e.text,
        Ok(None) => prompt,
        Err(e) => bail!("{e}"),
    };
    let out = agent.run(&prompt).await;
    let session_path = agent.session().path().to_owned();
    let ext = agent.extensions().clone();
    ext.notify(
        "session_shutdown",
        serde_json::json!({"session_file": session_path}),
    )
    .await;
    ext.shutdown();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_extensions_marked_unsandboxed_leave_the_sandbox() {
        let s = serde_json::json!({"extensions": {
            "deploy": {"unsandboxed": true},
            "lint": {"unsandboxed": false},
            "other": {}
        }});
        assert_eq!(unsandboxed_extensions(&s), ["deploy"]);
        assert!(unsandboxed_extensions(&serde_json::json!({})).is_empty());
    }
}
