#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! A real stdio MCP server (the `psi-mcp-fake` binary): started as a child
//! process the way users' servers are, with its env and cwd from the
//! config; a server that dies mid-session; a call that times out; Esc
//! during a call; the process stopped when psi lets go of it.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use psi_mcp::config::Transport;
use psi_mcp::testing::config;
use psi_mcp::{McpManager, McpTool, ServerConfig, State};
use serde_json::Map;
use tokio_util::sync::CancellationToken;

fn fake(name: &str, env: &[(&str, &str)], cwd: &Path) -> ServerConfig {
    let mut c = config(name);
    c.transport = Transport::Stdio {
        command: env!("CARGO_BIN_EXE_psi-mcp-fake").into(),
        args: vec![],
        env: env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        cwd: Some(cwd.to_owned()),
    };
    c
}

async fn started(c: ServerConfig) -> (Arc<McpManager>, Vec<Arc<McpTool>>) {
    let m = McpManager::start(vec![c], vec![], Path::new("."));
    m.wait(Duration::from_secs(10)).await;
    assert_eq!(
        m.status(&[])[0].state,
        State::Connected,
        "{:?}",
        m.status(&[])
    );
    let tools = m.tools(&[]);
    (m, tools)
}

fn tool<'a>(tools: &'a [Arc<McpTool>], remote: &str) -> &'a McpTool {
    tools.iter().find(|t| t.remote == remote).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stdio_server_gets_its_env_and_cwd_and_answers() {
    let dir = tempfile::tempdir().unwrap();
    let (m, tools) = started(fake("fake", &[("FAKE_VAR", "hello")], dir.path())).await;
    assert_eq!(tools.len(), 2);
    let (text, err) = m
        .call(
            tool(&tools, "echo_env"),
            Map::new(),
            &CancellationToken::new(),
            1000,
            dir.path(),
        )
        .await;
    assert!(!err, "{text}");
    assert_eq!(
        text,
        format!("hello in {}", dir.path().canonicalize().unwrap().display())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_dies_mid_session() {
    let dir = tempfile::tempdir().unwrap();
    let (m, tools) = started(fake("fake", &[("PSI_FAKE_EXIT_AFTER", "1")], dir.path())).await;
    let t = tool(&tools, "echo_env");
    let cancel = CancellationToken::new();
    assert!(!m.call(t, Map::new(), &cancel, 1000, dir.path()).await.1);
    let start = Instant::now();
    let (text, err) = m.call(t, Map::new(), &cancel, 1000, dir.path()).await;
    assert!(
        err && text.contains("stopped while psi was running"),
        "{text}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert!(matches!(m.status(&[])[0].state, State::Failed(_)));
    assert!(m.tools(&[]).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_that_times_out_leaves_the_server_usable() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = fake("fake", &[("FAKE_VAR", "hi")], dir.path());
    c.timeout = Duration::from_secs(1);
    let (m, tools) = started(c).await;
    let cancel = CancellationToken::new();
    let (text, err) = m
        .call(tool(&tools, "slow"), Map::new(), &cancel, 1000, dir.path())
        .await;
    assert!(
        err && text.contains("did not answer within 1 seconds"),
        "{text}"
    );
    // The server still answers once the slow call is done with.
    let mut ok = false;
    for _ in 0..10 {
        let (t, e) = m
            .call(
                tool(&tools, "echo_env"),
                Map::new(),
                &cancel,
                1000,
                dir.path(),
            )
            .await;
        if !e && t.starts_with("hi in") {
            ok = true;
            break;
        }
    }
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread")]
async fn esc_during_a_call_returns_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (m, tools) = started(fake("fake", &[], dir.path())).await;
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        c.cancel();
    });
    let start = Instant::now();
    let r = m
        .call(tool(&tools, "slow"), Map::new(), &cancel, 1000, dir.path())
        .await;
    assert_eq!(r, ("[aborted]".into(), true));
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_process_stops_when_psi_lets_go() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let (m, _) = started(fake(
        "fake",
        &[("PSI_FAKE_PID_FILE", pid_file.to_str().unwrap())],
        dir.path(),
    ))
    .await;
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    assert!(Path::new(&format!("/proc/{pid}")).exists());
    drop(m);
    let gone = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        // A zombie still has a /proc entry; its state is Z.
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map_or(true, |s| s.split_whitespace().nth(2) == Some("Z"))
    });
    assert!(gone, "the server process outlived its manager");
}
