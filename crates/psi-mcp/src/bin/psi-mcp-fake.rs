//! A real stdio MCP server for psi-mcp's tests (built with the `testing`
//! feature only). Tools: `echo_env` answers `$FAKE_VAR in <cwd>`, `slow`
//! answers after three seconds. `PSI_FAKE_EXIT_AFTER=n` exits on call n+1
//! (a server that dies mid-session); `PSI_FAKE_PID_FILE` gets its pid.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Map, Value, json};

#[tokio::main]
async fn main() {
    if let Ok(f) = std::env::var("PSI_FAKE_PID_FILE") {
        let _ = std::fs::write(f, std::process::id().to_string());
    }
    let exit_after: Option<usize> = std::env::var("PSI_FAKE_EXIT_AFTER")
        .ok()
        .and_then(|n| n.parse().ok());
    let calls = AtomicUsize::new(0);
    let tools = json!([
        {"name": "echo_env", "description": "Says FAKE_VAR and the directory.",
         "annotations": {"readOnlyHint": true},
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "slow", "description": "Answers after three seconds.",
         "annotations": {"readOnlyHint": true},
         "inputSchema": {"type": "object", "properties": {}}}
    ]);
    let answer = move |tool: &str, _: &Map<String, Value>| {
        let n = calls.fetch_add(1, Ordering::SeqCst);
        if exit_after.is_some_and(|max| n >= max) {
            std::process::exit(0);
        }
        let text = match tool {
            "slow" => {
                std::thread::sleep(std::time::Duration::from_secs(3));
                "late".to_owned()
            }
            _ => format!(
                "{} in {}",
                std::env::var("FAKE_VAR").unwrap_or_default(),
                std::env::current_dir()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default()
            ),
        };
        json!({"content": [{"type": "text", "text": text}]})
    };
    psi_mcp::testing::serve_stdio(tools, Arc::new(answer)).await;
}
