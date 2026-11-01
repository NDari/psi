//! A fake MCP server over an in-process pipe, for tests here and in the
//! agent: tools given as MCP JSON, calls answered by a closure.

use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig as ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer, ServiceExt};
use serde_json::{Map, Value};

use crate::config::{Approval, ServerConfig, Transport};
use crate::manager::{McpManager, start_with};

/// Answers a call: (tool, arguments) to an MCP `CallToolResult` as JSON.
pub type Answer = Arc<dyn Fn(&str, &Map<String, Value>) -> Value + Send + Sync>;

struct Fake {
    tools: Value,
    answer: Answer,
}

impl ServerHandler for Fake {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        serde_json::from_value(serde_json::json!({"tools": self.tools}))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))
    }

    async fn call_tool(
        &self,
        req: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = req.arguments.unwrap_or_default();
        let v = (self.answer)(&req.name, &args);
        let r: CallToolResult = serde_json::from_value(v)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResponse::Complete(r))
    }
}

/// A config for a server named `name` (the transport isn't used).
pub fn config(name: &str) -> ServerConfig {
    ServerConfig {
        name: name.into(),
        transport: Transport::Stdio {
            command: "fake".into(),
            args: vec![],
            env: vec![],
            cwd: None,
        },
        enabled: true,
        tools: None,
        disabled_tools: vec![],
        approval: Approval::Auto,
        tool_approval: Default::default(),
        timeout: std::time::Duration::from_secs(5),
        source: "mcp.json".into(),
    }
}

/// A manager connected to one fake server offering `tools` (a JSON list
/// of MCP tools).
pub async fn fake_manager(config: ServerConfig, tools: Value, answer: Answer) -> Arc<McpManager> {
    let (client_io, server_io) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        if let Ok(s) = (Fake { tools, answer }).serve(server_io).await {
            let _ = s.waiting().await;
        }
    });
    start_with(config, client_io).await
}
