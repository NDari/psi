#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! The manager against a fake server over an in-process pipe, and stdio
//! servers that fail to start.

use std::sync::Arc;
use std::time::Duration;

use psi_mcp::config::{Approval, Transport};
use psi_mcp::testing::{config, fake_manager};
use psi_mcp::{McpManager, State};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

fn tools() -> Value {
    json!([
        {"name": "search", "description": "Finds issues.", "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"]}, "annotations": {"readOnlyHint": true}},
        {"name": "close", "description": "Closes an issue.", "inputSchema": {"type": "object", "properties": {"id": {"type": "integer"}}}},
        {"name": "bad", "inputSchema": {"type": "string"}}
    ])
}

fn answer(tool: &str, args: &Map<String, Value>) -> Value {
    match tool {
        "search" => {
            json!({"content": [{"type": "text", "text": format!("2 issues match {}", args["q"])}]})
        }
        _ => json!({"content": [{"type": "text", "text": "no such issue"}], "isError": true}),
    }
}

#[tokio::test]
async fn tools_are_named_cleaned_and_called() {
    let m = fake_manager(config("git-hub"), tools(), Arc::new(answer)).await;
    let t = m.tools(&["read", "bash"]);
    let names: Vec<&str> = t.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["mcp__git_hub__search", "mcp__git_hub__close"]);
    assert!(t[0].read_only && !t[1].read_only);
    assert_eq!(t[0].approval, Approval::Auto);
    assert_eq!(t[0].schema.description, "Finds issues.");
    // The same tool objects each time: built once.
    assert!(Arc::ptr_eq(&t[0], &m.tools(&["read"])[0]));

    let dir = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let args: Map<String, Value> = serde_json::from_value(json!({"q": "crash"})).unwrap();
    let r = m.call(&t[0], args, &cancel, 1000, dir.path()).await;
    assert_eq!(r, ("2 issues match \"crash\"".into(), false));
    let r = m.call(&t[1], Map::new(), &cancel, 1000, dir.path()).await;
    assert_eq!(r, ("no such issue".into(), true));

    let st = &m.status(&[])[0];
    assert_eq!(st.state, State::Connected);
    assert_eq!(st.tools.len(), 2);
    assert!(st.tools.iter().all(|(_, tokens)| *tokens > 10));
    assert_eq!(
        st.skipped,
        [("bad".to_owned(), "its input isn't an object".to_owned())]
    );
}

#[tokio::test]
async fn the_config_lists_choose_the_tools() {
    let mut c = config("gh");
    c.disabled_tools = vec!["close".into()];
    let m = fake_manager(c, tools(), Arc::new(answer)).await;
    let names: Vec<String> = m.tools(&[]).iter().map(|t| t.name.clone()).collect();
    assert_eq!(names, ["mcp__gh__search"]);
    assert!(
        m.status(&[])[0]
            .skipped
            .iter()
            .any(|(n, why)| n == "close" && why.contains("disabledTools"))
    );
}

fn stdio(name: &str, command: &str, args: &[&str]) -> psi_mcp::ServerConfig {
    let mut c = config(name);
    c.transport = Transport::Stdio {
        command: command.into(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        env: vec![],
        cwd: None,
    };
    c
}

#[tokio::test]
async fn stdio_servers_that_fail_say_why() {
    let mut off = stdio("off", "true", &[]);
    off.enabled = false;
    let m = McpManager::start(
        vec![
            stdio("missing", "/nonexistent/psi-mcp-server", &[]),
            stdio("dies", "sh", &["-c", "echo 'bad token' >&2; exit 3"]),
            off,
        ],
        vec!["mcp.json: server 'x': skipped".into()],
        std::path::Path::new("."),
    );
    m.wait(Duration::from_secs(10)).await;
    let st = m.status(&[]);
    assert!(
        matches!(&st[0].state, State::Failed(e) if e.starts_with("cannot start '/nonexistent/psi-mcp-server'")),
        "{:?}",
        st[0].state
    );
    assert!(
        matches!(&st[1].state, State::Failed(_)),
        "{:?}",
        st[1].state
    );
    // Its stderr is kept for the report; give the reader a moment.
    for _ in 0..50 {
        if !m.status(&[])[1].stderr.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(m.status(&[])[1].stderr, ["bad token"]);
    assert_eq!(st[2].state, State::Disabled);
    assert!(m.tools(&[]).is_empty());
    assert_eq!(m.problems().len(), 1);
}

#[tokio::test]
async fn an_http_server_that_is_down_says_where() {
    // A port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut c = config("docs");
    c.transport = Transport::Http {
        url: format!("http://127.0.0.1:{port}/mcp"),
        headers: vec![("Authorization".into(), "Bearer t".into())],
    };
    let mut bad = config("bad");
    bad.transport = Transport::Http {
        url: "http://127.0.0.1:1/mcp".into(),
        headers: vec![("bad header".into(), "x".into())],
    };
    let m = McpManager::start(vec![c, bad], vec![], std::path::Path::new("."));
    m.wait(Duration::from_secs(20)).await;
    let st = m.status(&[]);
    assert!(
        matches!(&st[0].state, State::Failed(e) if e.starts_with(&format!("connecting to http://127.0.0.1:{port}/mcp failed"))),
        "{:?}",
        st[0].state
    );
    assert!(
        matches!(&st[1].state, State::Failed(e) if e.starts_with("header 'bad header'")),
        "{:?}",
        st[1].state
    );
}

#[tokio::test]
async fn failed_calls_are_counted() {
    let m = fake_manager(config("gh"), tools(), Arc::new(answer)).await;
    let t = m.tools(&[]);
    let dir = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let args: Map<String, Value> = serde_json::from_value(json!({"q": "x"})).unwrap();
    m.call(&t[0], args, &cancel, 1000, dir.path()).await;
    assert_eq!(m.status(&[])[0].failed_calls, 0);
    m.call(&t[1], Map::new(), &cancel, 1000, dir.path()).await;
    m.call(&t[1], Map::new(), &cancel, 1000, dir.path()).await;
    assert_eq!(m.status(&[])[0].failed_calls, 2);
}
