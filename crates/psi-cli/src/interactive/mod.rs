//! The interactive mode (psi-plan.md §11): an inline TUI over the same
//! agent as print mode. The UI runs on the main thread; the agent on the
//! tokio runtime; they talk over channels (§3.3).

mod approver;
mod complete;
mod paste;
mod ui;
mod view;
mod worker;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{execute, terminal};
use psi_safety::checkpoint::Checkpoints;
use psi_tui::theme::{self, Token};
use psi_tui::{Keymap, Line, Style};

use crate::run_mode::{Prepared, RunArgs, prepare};
use approver::TuiApprover;
use complete::Completer;
use ui::{App, AppParts, Notify};
use view::Status;
use worker::{Worker, forward};

/// Prompts kept in ~/.psi/history.
const HISTORY: usize = 500;

fn load_history(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<String>(l).ok())
        .collect()
}

fn save_history(path: &Path, h: &[String]) {
    let start = h.len().saturating_sub(HISTORY);
    let text: String = h[start..]
        .iter()
        .filter_map(|s| serde_json::to_string(s).ok())
        .map(|l| l + "\n")
        .collect();
    if let Err(e) = std::fs::write(path, text) {
        tracing::warn!("cannot save the prompt history: {e}");
    }
}

/// `~/.psi/keybindings.json` over the defaults (pi parity); problems are
/// startup warnings.
fn load_keymap(home: Option<&Path>, warnings: &mut Vec<String>) -> Keymap {
    let Some(path) = home.map(|h| h.join(".psi/keybindings.json")) else {
        return Keymap::default();
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Keymap::default(),
    };
    match psi_cli::config::parse_jsonc(&text, &path.display().to_string()) {
        Ok(v) => {
            let (km, problems) = Keymap::from_json(&v);
            warnings.extend(problems);
            km
        }
        Err(e) => {
            warnings.push(format!("{e:#}; using the default keys"));
            Keymap::default()
        }
    }
}

/// Puts the terminal back, also when psi panics.
/// Raw mode, bracketed paste, focus events and the kitty keyboard flags.
fn enter_terminal() -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut out = std::io::stdout();
    let _ = execute!(out, EnableBracketedPaste, EnableFocusChange);
    // Shift+Enter needs the kitty keyboard protocol; terminals without it
    // ignore the request. Asking first would wait for an answer some
    // terminals never send (up to 2 s of startup).
    let _ = execute!(
        out,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    Ok(())
}

/// Hands the terminal to another program (an editor) until it exits, then
/// takes it back. The caller clears and redraws the live region around it.
fn with_terminal_released<T>(f: impl FnOnce() -> T) -> T {
    restore_terminal();
    let out = f();
    let _ = enter_terminal();
    out
}

/// Removes the progress indicator and puts the title back (after a panic;
/// a normal exit does this in `App::finish`).
fn clear_terminal_status() {
    let t = psi_tui::term::Term::detect(|k| std::env::var(k).ok());
    let mut bytes = t.progress(psi_tui::term::Activity::Idle);
    bytes.extend(t.pop_title());
    let mut out = std::io::stdout();
    let _ = out.write_all(&bytes);
}

fn restore_terminal() {
    let mut out = std::io::stdout();
    let _ = execute!(
        out,
        PopKeyboardEnhancementFlags,
        DisableBracketedPaste,
        DisableFocusChange
    );
    let _ = terminal::disable_raw_mode();
    let _ = out.flush();
}

pub fn run(args: RunArgs) -> Result<ExitCode> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    let (ui_tx, mut ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let approver = Arc::new(TuiApprover { ui: ui_tx.clone() });
    let Prepared {
        agent,
        profile,
        n_ctx,
        tmpdir,
        cwd,
        home,
        sessions,
        warnings,
        info,
        mut ext_requests,
        ext_sources,
        resources,
    } = rt.block_on(prepare(&args, approver))?;
    let ext = agent.extensions().clone();
    let plan_on = agent.plan().enabled;
    let fwd = ui_tx.clone();
    rt.spawn(async move {
        while let Some((name, r)) = ext_requests.recv().await {
            if fwd.send(ui::UiMsg::Ext(name, r)).is_err() {
                break;
            }
        }
    });

    let (ev_tx, ev_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = agent.with_events(ev_tx);
    rt.spawn(forward(ev_rx, ui_tx.clone()));
    let queues = agent.queues();
    let cancel = agent.abort_handle();
    let resumed = agent.session().entries().to_vec();
    let session_path = agent.session().path().to_owned();
    let root = agent.safety().policy.root.clone();
    let store = home.as_deref().map(|h| h.join(".psi/checkpoints"));
    let cp_root = root.clone();
    let open_checkpoints = Box::new(move |s: &psi_session::SessionFile| {
        let id = s.path().file_stem()?.to_string_lossy().into_owned();
        Checkpoints::open(store.as_deref()?, &cp_root, &id)
            .ok()
            .map(Arc::new)
    });
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let worker = Worker {
        agent,
        sessions_root: sessions.clone(),
        cwd: cwd.clone(),
        open_checkpoints,
        ext_sources,
    };
    rt.spawn(worker.run(cmd_rx, ui_tx.clone()));

    let history_path = home.as_deref().map(|h| h.join(".psi/history"));
    let history = history_path
        .as_deref()
        .map(load_history)
        .unwrap_or_default();
    let status = Status {
        model: profile.id.clone(),
        n_ctx,
        mode: info["mode"].as_str().unwrap_or("auto").into(),
        network: info["network"].as_str().unwrap_or("on").into(),
        unisolated: info["isolation"] == "unavailable",
        plan: plan_on.then(|| "plan".to_owned()),
        ..Status::default()
    };

    let mut warnings = warnings;
    let keymap = load_keymap(home.as_deref(), &mut warnings);
    let wanted = args.settings["theme"].as_str().unwrap_or("default");
    match resources.theme(wanted) {
        Some(t) => theme::set(t),
        None => warnings.push(format!(
            "no theme named '{wanted}' (in ~/.psi/themes); using the default"
        )),
    }
    let notify = Notify::from_setting(&args.settings["notify"]).unwrap_or_else(|e| {
        warnings.push(e);
        Notify::Auto
    });
    let (w, h) = terminal::size().unwrap_or((80, 24));
    enter_terminal().context("the terminal can't go into raw mode")?;
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |p| {
        clear_terminal_status();
        restore_terminal();
        prev(p);
    }));

    let mut app = App::new(AppParts {
        out: std::io::stdout(),
        width: w,
        height: h,
        status,
        cmds: cmd_tx,
        queues,
        cancel,
        completer: Completer::new(&cwd),
        sessions_dir: sessions.join(psi_session::cwd_slug(&cwd)),
        session_path: session_path.clone(),
        history,
        ext: ext.clone(),
        rt: rt.handle().clone(),
        ui_tx,
        resources,
        keymap,
        term: psi_tui::term::Term::detect(|k| std::env::var(k).ok()),
        notify,
        dir: cwd
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| cwd.display().to_string()),
    });
    let mut banner = vec![Line::styled(
        format!(
            "ψ psi {}  ·  {}  ·  /help",
            env!("CARGO_PKG_VERSION"),
            cwd.display()
        ),
        Style::default().dim(),
    )];
    for w in &warnings {
        banner.push(Line::styled(format!("  {w}"), theme::style(Token::Warning)));
    }
    let result = app
        .start(banner)
        .and_then(|()| app.replay(&resumed))
        .and_then(|()| app.run(&mut ui_rx));
    let _ = app.finish();
    restore_terminal();
    if let Some(p) = &history_path {
        save_history(p, &app.history());
    }
    drop(app);
    let file = session_path.display().to_string();
    rt.block_on(ext.notify(
        "session_shutdown",
        serde_json::json!({"session_file": file}),
    ));
    ext.shutdown();
    rt.shutdown_timeout(std::time::Duration::from_millis(500));
    let _ = std::fs::remove_dir_all(&tmpdir);
    result.context("the terminal failed")?;
    println!("session: {}", session_path.display());
    Ok(ExitCode::SUCCESS)
}
