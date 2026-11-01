//! Connections to the configured servers (psi-plan.md §10.2): each enabled
//! server connects in the background when the session starts; the agent
//! waits a little for them before its first request, then takes the tools
//! as they are at the start of each run.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use process_wrap::tokio::{CommandWrap, ProcessGroup};
use psi_ai::ToolSchema;
use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation};
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use rmcp::transport::TokioChildProcess;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};
use serde_json::{Map, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::config::{Approval, ServerConfig, Transport};
use crate::names;
use crate::result::render;
use crate::schema::{clean, tool_schema};

/// Starting a server, initializing and listing its tools.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Lines of a stdio server's stderr kept for `psi mcp list` and `/mcp`:
/// enough that the cause of a failure is still there after a few failed
/// calls have logged their own lines (a bridge whose language server died
/// at start, 2026-11-01).
const STDERR_LINES: usize = 40;

type Service = RunningService<RoleClient, ClientConfig>;

/// A server tool as offered to the model.
#[derive(Debug)]
pub struct McpTool {
    /// `mcp__<server>__<tool>`
    pub name: String,
    pub server: String,
    /// The server's own name for it.
    pub remote: String,
    pub schema: ToolSchema,
    /// The server marks it read-only (`readOnlyHint`).
    pub read_only: bool,
    pub approval: Approval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Disabled,
    Connecting,
    Connected,
    Failed(String),
}

/// One server for `psi mcp list` and `/mcp`.
#[derive(Debug, Clone)]
pub struct ServerStatus {
    pub name: String,
    pub source: PathBuf,
    pub state: State,
    /// The tools offered, with their schema's size in tokens (estimated).
    pub tools: Vec<(String, usize)>,
    /// Tools left out, and why (the config's lists, an unusable schema).
    pub skipped: Vec<(String, String)>,
    pub stderr: Vec<String>,
    /// Calls that failed: `isError`, a protocol error or no answer.
    pub failed_calls: usize,
}

struct RemoteTool {
    remote: String,
    description: String,
    schema: Value,
    read_only: bool,
}

enum Conn {
    Disabled,
    Connecting,
    Connected {
        service: Arc<Service>,
        tools: Vec<RemoteTool>,
        skipped: Vec<(String, String)>,
    },
    Failed(String),
}

struct Server {
    config: ServerConfig,
    conn: Mutex<Conn>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    failed_calls: AtomicUsize,
}

pub struct McpManager {
    servers: Vec<Arc<Server>>,
    problems: Vec<String>,
    pending: watch::Receiver<usize>,
    /// Built tools by (name, server, remote): their strings are leaked, so
    /// each is built once.
    built: Mutex<HashMap<(String, String, String), Arc<McpTool>>>,
}

impl McpManager {
    /// No servers.
    pub fn empty() -> Arc<Self> {
        Self::start(vec![], vec![], Path::new("."))
    }

    /// Connects every enabled server in the background. `problems` are the
    /// config's, reported with the servers'.
    pub fn start(configs: Vec<ServerConfig>, problems: Vec<String>, cwd: &Path) -> Arc<Self> {
        let enabled = configs.iter().filter(|c| c.enabled).count();
        let (tx, rx) = watch::channel(enabled);
        let servers: Vec<Arc<Server>> = configs
            .into_iter()
            .map(|config| {
                let conn = if config.enabled {
                    Conn::Connecting
                } else {
                    Conn::Disabled
                };
                Arc::new(Server {
                    config,
                    conn: Mutex::new(conn),
                    stderr: Arc::default(),
                    failed_calls: AtomicUsize::new(0),
                })
            })
            .collect();
        for s in servers.iter().filter(|s| s.config.enabled) {
            let (s, tx, cwd) = (s.clone(), tx.clone(), cwd.to_owned());
            tokio::spawn(async move {
                let conn = match tokio::time::timeout(CONNECT_TIMEOUT, connect(&s, &cwd)).await {
                    Ok(Ok(c)) => c,
                    Ok(Err(e)) => Conn::Failed(e),
                    Err(_) => Conn::Failed(format!(
                        "no answer within {} seconds",
                        CONNECT_TIMEOUT.as_secs()
                    )),
                };
                if let Conn::Failed(e) = &conn {
                    tracing::info!("MCP server '{}' failed: {e}", s.config.name);
                }
                if let Ok(mut c) = s.conn.lock() {
                    *c = conn;
                }
                tx.send_modify(|n| *n = n.saturating_sub(1));
            });
        }
        Arc::new(Self {
            servers,
            problems,
            pending: rx,
            built: Mutex::default(),
        })
    }

    /// Whether any server is configured.
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// Waits until every server connected or failed, or `limit` passed.
    pub async fn wait(&self, limit: Duration) {
        let mut rx = self.pending.clone();
        let _ = tokio::time::timeout(limit, rx.wait_for(|n| *n == 0)).await;
    }

    /// The tools of the connected servers, in config order, named so none
    /// clashes with `taken` (the core and extension tools).
    pub fn tools(&self, taken: &[&str]) -> Vec<Arc<McpTool>> {
        struct Pick<'a> {
            server: &'a ServerConfig,
            remote: String,
            description: String,
            schema: Value,
            read_only: bool,
        }
        let mut picks = Vec::new();
        for s in &self.servers {
            let Ok(conn) = s.conn.lock() else { continue };
            if let Conn::Connected { tools, .. } = &*conn {
                for t in tools.iter().filter(|t| s.config.offers(&t.remote)) {
                    picks.push(Pick {
                        server: &s.config,
                        remote: t.remote.clone(),
                        description: t.description.clone(),
                        schema: t.schema.clone(),
                        read_only: t.read_only,
                    });
                }
            }
        }
        let pairs: Vec<(&str, &str)> = picks
            .iter()
            .map(|p| (p.server.name.as_str(), p.remote.as_str()))
            .collect();
        let names = names::assign(&pairs, taken);
        let Ok(mut built) = self.built.lock() else {
            return vec![];
        };
        picks
            .into_iter()
            .zip(names)
            .map(|(p, name)| {
                let key = (name.clone(), p.server.name.clone(), p.remote.clone());
                built
                    .entry(key)
                    .or_insert_with(|| {
                        Arc::new(McpTool {
                            schema: tool_schema(&name, &p.description, &p.schema),
                            name,
                            server: p.server.name.clone(),
                            approval: p.server.approval_for(&p.remote),
                            remote: p.remote,
                            read_only: p.read_only,
                        })
                    })
                    .clone()
            })
            .collect()
    }

    /// Runs a tool; the model's text and whether it failed. Results past
    /// `max_chars` are saved whole in `spill_dir`.
    pub async fn call(
        &self,
        tool: &McpTool,
        args: Map<String, Value>,
        cancel: &CancellationToken,
        max_chars: usize,
        spill_dir: &Path,
    ) -> (String, bool) {
        let Some(server) = self.servers.iter().find(|s| s.config.name == tool.server) else {
            return (format!("There is no MCP server '{}'.", tool.server), true);
        };
        let service = match server.conn.lock().as_deref() {
            Ok(Conn::Connected { service, .. }) => service.clone(),
            Ok(Conn::Failed(e)) => {
                return (
                    format!(
                        "Not run: MCP server '{}' is not connected ({e}).",
                        tool.server
                    ),
                    true,
                );
            }
            _ => {
                return (
                    format!("Not run: MCP server '{}' is not connected.", tool.server),
                    true,
                );
            }
        };
        let mut params = CallToolRequestParams::new(tool.remote.clone());
        params.arguments = Some(args);
        let timeout = server.config.timeout;
        let result = tokio::select! {
            r = tokio::time::timeout(timeout, service.call_tool(params)) => r,
            () = cancel.cancelled() => return ("[aborted]".into(), true),
        };
        let out = match result {
            Ok(Ok(r)) => match serde_json::to_value(&r) {
                Ok(v) => render(&v, max_chars, spill_dir, &tool.name),
                Err(e) => (
                    format!(
                        "MCP server '{}' sent a result Psi can't read: {e}",
                        tool.server
                    ),
                    true,
                ),
            },
            Ok(Err(e)) => (format!("MCP server '{}' failed: {e}", tool.server), true),
            Err(_) => (
                format!(
                    "MCP server '{}' did not answer within {} seconds.",
                    tool.server,
                    timeout.as_secs()
                ),
                true,
            ),
        };
        if out.1 {
            server.failed_calls.fetch_add(1, Ordering::Relaxed);
        }
        out
    }

    /// Each server's state and tools.
    pub fn status(&self, taken: &[&str]) -> Vec<ServerStatus> {
        let offered = self.tools(taken);
        self.servers
            .iter()
            .map(|s| {
                let (state, skipped) = match s.conn.lock().as_deref() {
                    Ok(Conn::Disabled) => (State::Disabled, vec![]),
                    Ok(Conn::Connecting) => (State::Connecting, vec![]),
                    Ok(Conn::Connected { tools, skipped, .. }) => {
                        let mut skipped = skipped.clone();
                        for t in tools.iter().filter(|t| !s.config.offers(&t.remote)) {
                            skipped
                                .push((t.remote.clone(), "left out by tools/disabledTools".into()));
                        }
                        (State::Connected, skipped)
                    }
                    Ok(Conn::Failed(e)) => (State::Failed(e.clone()), vec![]),
                    Err(_) => (State::Failed("internal error".into()), vec![]),
                };
                let tools = offered
                    .iter()
                    .filter(|t| t.server == s.config.name)
                    .map(|t| (t.name.clone(), t.schema.to_openai().to_string().len() / 3))
                    .collect();
                ServerStatus {
                    name: s.config.name.clone(),
                    source: s.config.source.clone(),
                    state,
                    tools,
                    skipped,
                    stderr: s
                        .stderr
                        .lock()
                        .map(|l| l.iter().cloned().collect())
                        .unwrap_or_default(),
                    failed_calls: s.failed_calls.load(Ordering::Relaxed),
                }
            })
            .collect()
    }
}

fn client_info() -> ClientConfig {
    ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("psi", env!("CARGO_PKG_VERSION")),
    )
}

async fn connect(s: &Server, cwd: &Path) -> Result<Conn, String> {
    let service = match &s.config.transport {
        Transport::Stdio {
            command,
            args,
            env,
            cwd: dir,
        } => {
            let mut cmd = tokio::process::Command::new(command);
            cmd.args(args)
                .envs(env.iter().map(|(k, v)| (k, v)))
                .current_dir(dir.as_deref().unwrap_or(cwd));
            // Its own process group, so wrappers like npx stop with it.
            let mut wrap = CommandWrap::from(cmd);
            wrap.wrap(ProcessGroup::leader());
            let (proc, stderr) = TokioChildProcess::builder(wrap)
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("cannot start '{command}': {e}"))?;
            if let Some(err) = stderr {
                let (tail, name) = (s.stderr.clone(), s.config.name.clone());
                tokio::spawn(async move {
                    let mut lines = BufReader::new(err).lines();
                    while let Ok(Some(l)) = lines.next_line().await {
                        tracing::debug!("MCP server '{name}': {l}");
                        if let Ok(mut t) = tail.lock() {
                            if t.len() == STDERR_LINES {
                                t.pop_front();
                            }
                            t.push_back(l);
                        }
                    }
                });
            }
            client_info()
                .serve(proc)
                .await
                .map_err(|e| format!("it didn't start as an MCP server: {e}"))?
        }
        Transport::Http { url, headers } => {
            let mut h = HashMap::new();
            for (k, v) in headers {
                let name = reqwest::header::HeaderName::try_from(k.as_str())
                    .map_err(|e| format!("header '{k}': {e}"))?;
                let value = reqwest::header::HeaderValue::try_from(v.as_str())
                    .map_err(|e| format!("header '{k}': {e}"))?;
                h.insert(name, value);
            }
            let config =
                StreamableHttpClientTransportConfig::with_uri(url.clone()).custom_headers(h);
            client_info()
                .serve(StreamableHttpClientTransport::from_config(config))
                .await
                .map_err(|e| format!("connecting to {url} failed: {e}"))?
        }
    };
    let listed = service
        .list_all_tools()
        .await
        .map_err(|e| format!("listing its tools failed: {e}"))?;
    let (tools, skipped) = remote_tools(listed);
    Ok(Conn::Connected {
        service: Arc::new(service),
        tools,
        skipped,
    })
}

/// The tools Psi can offer, and those skipped with why.
fn remote_tools(listed: Vec<rmcp::model::Tool>) -> (Vec<RemoteTool>, Vec<(String, String)>) {
    let mut tools = Vec::new();
    let mut skipped = Vec::new();
    for t in listed {
        let v = serde_json::to_value(&t).unwrap_or(Value::Null);
        let name = t.name.to_string();
        match clean(&v["inputSchema"]) {
            Ok(schema) => tools.push(RemoteTool {
                remote: name,
                description: t.description.as_deref().unwrap_or("").to_owned(),
                schema,
                read_only: v["annotations"]["readOnlyHint"].as_bool() == Some(true),
            }),
            Err(e) => skipped.push((name, e)),
        }
    }
    (tools, skipped)
}

/// Connect to a server over an in-process pipe (tests).
#[doc(hidden)]
pub async fn start_with<T, E, A>(config: ServerConfig, transport: T) -> Arc<McpManager>
where
    T: rmcp::transport::IntoTransport<RoleClient, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let (_tx, rx) = watch::channel(0);
    let conn = match client_info().serve(transport).await {
        Ok(service) => match service.list_all_tools().await {
            Ok(listed) => {
                let (tools, skipped) = remote_tools(listed);
                Conn::Connected {
                    service: Arc::new(service),
                    tools,
                    skipped,
                }
            }
            Err(e) => Conn::Failed(e.to_string()),
        },
        Err(e) => Conn::Failed(e.to_string()),
    };
    Arc::new(McpManager {
        servers: vec![Arc::new(Server {
            config,
            conn: Mutex::new(conn),
            stderr: Arc::default(),
            failed_calls: AtomicUsize::new(0),
        })],
        problems: vec![],
        pending: rx,
        built: Mutex::default(),
    })
}

/// What to tell the user once the servers had their time to connect:
/// failures, servers still connecting, and tool definitions that take more
/// than a tenth of the window (naming the biggest servers).
pub fn notices(status: &[ServerStatus], n_ctx: u64) -> Vec<String> {
    let mut out = Vec::new();
    for s in status {
        match &s.state {
            State::Failed(e) => out.push(format!(
                "MCP server '{}' failed: {e}. `psi mcp list` shows more.",
                s.name
            )),
            State::Connecting => out.push(format!(
                "MCP server '{}' is still connecting; its tools join at the next prompt.",
                s.name
            )),
            State::Connected | State::Disabled => {}
        }
    }
    let mut sizes: Vec<(&str, usize)> = status
        .iter()
        .map(|s| (s.name.as_str(), s.tools.iter().map(|(_, t)| t).sum()))
        .filter(|(_, t)| *t > 0)
        .collect();
    let total: usize = sizes.iter().map(|(_, t)| t).sum();
    if total as u64 > n_ctx / 10 {
        sizes.sort_by_key(|s| std::cmp::Reverse(s.1));
        let biggest: Vec<String> = sizes
            .iter()
            .take(3)
            .map(|(n, t)| format!("{n} about {t}"))
            .collect();
        out.push(format!(
            "MCP tools take about {total} tokens of the {n_ctx}-token context in every request ({}). Trim them with \"tools\" or \"disabledTools\" in mcp.json.",
            biggest.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(name: &str, state: State, tools: &[usize]) -> ServerStatus {
        ServerStatus {
            name: name.into(),
            source: "m.json".into(),
            state,
            tools: tools.iter().map(|t| ("t".to_owned(), *t)).collect(),
            skipped: vec![],
            stderr: vec![],
            failed_calls: 0,
        }
    }

    #[test]
    fn notices_name_failures_and_big_tool_lists() {
        let s = [
            st("a", State::Failed("boom".into()), &[]),
            st("b", State::Connecting, &[]),
            st("gh", State::Connected, &[3000, 2000]),
            st("fs", State::Connected, &[1000]),
        ];
        let n = notices(&s, 32_768);
        assert_eq!(
            n[0],
            "MCP server 'a' failed: boom. `psi mcp list` shows more."
        );
        assert!(n[1].starts_with("MCP server 'b' is still connecting"));
        assert_eq!(
            n[2],
            "MCP tools take about 6000 tokens of the 32768-token context in every request (gh about 5000, fs about 1000). Trim them with \"tools\" or \"disabledTools\" in mcp.json."
        );
        assert!(notices(&s[2..], 65_536).is_empty());
    }
}
