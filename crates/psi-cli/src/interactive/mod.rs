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

/// Puts the terminal back, also when psi panics.
/// Whether an executable called `name` is on PATH.
fn on_path(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| {
            std::fs::metadata(d.join(name))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

/// A startup line when rg or fd is missing: models search with them through
/// bash (Debian names fd `fdfind`), and slower `grep -r` walks ignored
/// directories. Says how to install them on this system.
fn missing_search_tools(found: impl Fn(&str) -> bool) -> Option<String> {
    let mut missing = Vec::new();
    if !found("rg") {
        missing.push("rg (ripgrep)");
    }
    if !found("fd") && !found("fdfind") {
        missing.push("fd");
    }
    if missing.is_empty() {
        return None;
    }
    let install = if cfg!(target_os = "macos") {
        "brew install ripgrep fd"
    } else if Path::new("/usr/bin/pacman").exists() {
        "sudo pacman -S ripgrep fd"
    } else if Path::new("/usr/bin/apt-get").exists() {
        "sudo apt install ripgrep fd-find"
    } else {
        "install ripgrep and fd"
    };
    Some(format!(
        "not found on PATH: {}; searches will be slower ({install})",
        missing.join(", ")
    ))
}

/// A startup line when tmux won't pass Shift+Enter on: with `extended-keys`
/// off it arrives as a plain Enter and sends the prompt (pi warns too).
fn tmux_extended_keys(
    in_tmux: bool,
    option: impl FnOnce(&str) -> Option<String>,
) -> Option<String> {
    if !in_tmux || option("extended-keys")?.trim() != "off" {
        return None;
    }
    Some(
        "tmux extended-keys is off, so Shift+Enter sends the prompt; Ctrl+J adds a new line. \
         To fix, in ~/.tmux.conf: set -g extended-keys on; set -g extended-keys-format csi-u; \
         set -as terminal-features 'xterm*:extkeys'"
            .into(),
    )
}

/// A global tmux option's value (`tmux show -gv`), or None when tmux can't say.
fn tmux_option(name: &str) -> Option<String> {
    let out = std::process::Command::new("tmux")
        .args(["show", "-gv", name])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

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

/// Asks the model server for its facts every two seconds until it answers,
/// then tells the UI (the TUI started without one).
async fn wait_for_server(
    profile: psi_ai::profile::ModelProfile,
    ui: tokio::sync::mpsc::UnboundedSender<ui::UiMsg>,
) {
    let client = psi_ai::Client::new(&profile.base_url).with_headers(&profile.headers);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let props = client.props().await;
        if crate::run_mode::not_up(&props) {
            continue;
        }
        let msg = match crate::run_mode::server_facts(&profile, props, true) {
            Ok((n_ctx, server, progress)) => ui::UiMsg::Connected {
                n_ctx,
                server,
                progress,
            },
            Err(e) => ui::UiMsg::Error(format!("  {e:#}")),
        };
        let _ = ui.send(msg);
        return;
    }
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
        offline,
        tmpdir,
        cwd,
        home,
        sessions,
        warnings,
        info,
    } = rt.block_on(prepare(&args, approver))?;

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
        offline: offline.then(|| profile.base_url.clone()),
        mode: info["mode"].as_str().unwrap_or("auto").into(),
        network: info["network"].as_str().unwrap_or("on").into(),
        unisolated: info["isolation"] == "unavailable",
        ..Status::default()
    };

    let mut warnings = warnings;
    if offline {
        warnings.push(format!(
            "no model server at {} yet: sessions, settings and commands work, and Psi connects when it is up (just llm-up)",
            profile.base_url
        ));
        rt.spawn(wait_for_server(profile.clone(), ui_tx.clone()));
    }
    if let Some(w) = missing_search_tools(on_path) {
        warnings.push(w);
    }
    let in_tmux = std::env::var_os("TMUX").is_some_and(|t| !t.is_empty());
    if let Some(w) = tmux_extended_keys(in_tmux, tmux_option) {
        warnings.push(w);
    }
    let keymap = Keymap::default();
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
        ui_tx,
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
    rt.shutdown_timeout(std::time::Duration::from_millis(500));
    let _ = std::fs::remove_dir_all(&tmpdir);
    result.context("the terminal failed")?;
    println!("session: {}", session_path.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::{missing_search_tools, tmux_extended_keys};

    #[test]
    fn tmux_with_extended_keys_off_gives_the_fix() {
        let off = |_: &str| Some("off\n".to_owned());
        let w = tmux_extended_keys(true, off).unwrap();
        assert!(w.contains("Ctrl+J adds a new line"), "{w}");
        assert!(w.contains("set -g extended-keys on"), "{w}");
        assert_eq!(tmux_extended_keys(false, off), None);
        assert_eq!(tmux_extended_keys(true, |_| Some("on\n".into())), None);
        assert_eq!(tmux_extended_keys(true, |_| None), None);
    }

    #[test]
    fn missing_rg_or_fd_gives_one_line_with_the_fix() {
        assert_eq!(missing_search_tools(|_| true), None);
        // Debian's fd is fdfind.
        assert_eq!(missing_search_tools(|n| n != "fd"), None);
        let w = missing_search_tools(|n| n == "fd").unwrap();
        assert!(w.starts_with("not found on PATH: rg (ripgrep);"), "{w}");
        let w = missing_search_tools(|_| false).unwrap();
        assert!(w.contains("rg (ripgrep), fd"), "{w}");
    }
}
