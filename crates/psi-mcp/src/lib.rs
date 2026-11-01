//! MCP servers (psi-plan.md §10.2, D-42): `mcp.json`, connections over
//! stdio, and the tools they offer as Psi tools.

pub mod config;
pub mod manager;
pub mod names;
pub mod result;
pub mod schema;
#[cfg(feature = "testing")]
pub mod testing;

pub use config::{Approval, ServerConfig, Transport};
pub use manager::{McpManager, McpTool, ServerStatus, State, notices};
