//! MCP servers from `mcp.json` (psi-plan.md §10.2): `~/.psi/mcp.json`,
//! then `.psi/mcp.json` in a trusted project, which replaces a user entry
//! of the same name.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use psi_mcp::McpManager;

pub fn files(home: Option<&Path>, cwd: &Path, trusted: bool) -> Vec<PathBuf> {
    let mut f: Vec<PathBuf> = home.map(|h| h.join(".psi/mcp.json")).into_iter().collect();
    if trusted {
        f.push(cwd.join(".psi/mcp.json"));
    }
    f
}

/// Reads the files and starts connecting every enabled server.
pub fn start(home: Option<&Path>, cwd: &Path, trusted: bool) -> Arc<McpManager> {
    let (servers, problems) =
        psi_mcp::config::load(&files(home, cwd, trusted), &|k| std::env::var(k).ok());
    McpManager::start(servers, problems, cwd)
}

/// Each server's state, tools and their size, what was left out, and the
/// stderr of one that failed; the config's problems first. For
/// `psi mcp list` and `/mcp`.
pub fn report(mcp: &McpManager) -> String {
    let taken: Vec<&str> = psi_tools::core_tools(true)
        .iter()
        .map(|t| t.name())
        .collect();
    format_report(mcp.problems(), &mcp.status(&taken))
}

/// The report from the config's problems and each server's status. A
/// server's stderr is shown when it failed to connect or a call to it
/// failed: a server can stay connected while its calls fail (a
/// language-server bridge whose language server died, 2026-11-01).
pub fn format_report(problems: &[String], status: &[psi_mcp::ServerStatus]) -> String {
    use psi_mcp::State;
    let mut out: Vec<String> = problems.iter().map(|p| format!("problem: {p}")).collect();
    if status.is_empty() && out.is_empty() {
        return "no MCP servers: add them to ~/.psi/mcp.json or .psi/mcp.json (`psi help mcp`)"
            .into();
    }
    for s in status {
        let total: usize = s.tools.iter().map(|(_, t)| t).sum();
        let failed = match s.failed_calls {
            0 => String::new(),
            1 => ", 1 call failed".into(),
            n => format!(", {n} calls failed"),
        };
        let state = match &s.state {
            State::Connected => format!(
                "connected, {} tools, about {total} tokens{failed}",
                s.tools.len()
            ),
            State::Connecting => "still connecting".into(),
            State::Disabled => "disabled".into(),
            State::Failed(e) => format!("failed: {e}"),
        };
        out.push(format!("{} ({}): {state}", s.name, s.source.display()));
        for (name, tokens) in &s.tools {
            out.push(format!("  {name}  about {tokens} tokens"));
        }
        for (name, why) in &s.skipped {
            out.push(format!("  left out: {name} ({why})"));
        }
        let failing = matches!(s.state, State::Failed(_)) || s.failed_calls > 0;
        if failing && !s.stderr.is_empty() {
            out.push("  stderr:".into());
            out.extend(s.stderr.iter().map(|l| format!("    {l}")));
        }
    }
    out.join("\n")
}

/// `psi mcp list`: connects every enabled server, waits for them, and
/// reports; false when a server failed or the config has problems.
pub async fn list(home: Option<&Path>, cwd: &Path, trusted: bool) -> (String, bool) {
    let mcp = start(home, cwd, trusted);
    mcp.wait(std::time::Duration::from_secs(35)).await;
    let taken: Vec<&str> = psi_tools::core_tools(true)
        .iter()
        .map(|t| t.name())
        .collect();
    let ok = mcp.problems().is_empty()
        && mcp.status(&taken).iter().all(|s| {
            matches!(
                s.state,
                psi_mcp::State::Connected | psi_mcp::State::Disabled
            )
        });
    (report(&mcp), ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use psi_mcp::{ServerStatus, State};

    fn st(failed_calls: usize) -> ServerStatus {
        ServerStatus {
            name: "lsp".into(),
            source: "/p/.psi/mcp.json".into(),
            state: State::Connected,
            tools: vec![("mcp__lsp__references".into(), 136)],
            skipped: vec![("edit_file".into(), "left out by tools/disabledTools".into())],
            stderr: vec!["malformed LSP payload".into()],
            failed_calls,
        }
    }

    #[test]
    fn stderr_shows_when_a_call_failed() {
        assert_eq!(
            format_report(&[], &[st(0)]),
            "lsp (/p/.psi/mcp.json): connected, 1 tools, about 136 tokens\n  mcp__lsp__references  about 136 tokens\n  left out: edit_file (left out by tools/disabledTools)"
        );
        let failing = format_report(&["bad entry".into()], &[st(2)]);
        assert!(
            failing.starts_with("problem: bad entry\nlsp (/p/.psi/mcp.json): connected, 1 tools, about 136 tokens, 2 calls failed\n"),
            "{failing}"
        );
        assert!(
            failing.ends_with("  stderr:\n    malformed LSP payload"),
            "{failing}"
        );
    }

    #[test]
    fn the_project_file_only_when_trusted() {
        let (home, cwd) = (Path::new("/h"), Path::new("/p"));
        assert_eq!(
            files(Some(home), cwd, false),
            [PathBuf::from("/h/.psi/mcp.json")]
        );
        assert_eq!(
            files(Some(home), cwd, true),
            [
                PathBuf::from("/h/.psi/mcp.json"),
                PathBuf::from("/p/.psi/mcp.json")
            ]
        );
    }
}
