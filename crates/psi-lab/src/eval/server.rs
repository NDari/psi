//! A minimal HTTP GET for llama-server's `/health` and `/props`.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::Value;

use crate::Result;
use crate::error::other;

/// GETs `path` from `127.0.0.1:port` and returns (status, body).
pub(crate) fn get(port: u16, path: &str) -> Result<(u16, String)> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let unreachable = |e: std::io::Error| other(format!("http://{addr}{path}: {e}"));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(unreachable)?;
    s.set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(unreachable)?;
    let req = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    s.write_all(req.as_bytes()).map_err(unreachable)?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(unreachable)?;
    let raw = String::from_utf8_lossy(&raw);
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| other(format!("http://{addr}{path}: malformed response")))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| other(format!("http://{addr}{path}: no status line")))?;
    Ok((status, body.to_owned()))
}

/// What the runner records about the model server.
#[derive(Debug, Clone)]
pub(crate) struct ServerInfo {
    pub n_ctx: u64,
    pub props: Value,
}

/// Checks `/health` and reads `/props`, refusing a template without tool calls.
pub(crate) fn probe(port: u16) -> Result<ServerInfo> {
    let (code, body) = get(port, "/health")?;
    if code != 200 {
        return Err(other(format!(
            "llama-server on port {port} is not ready: /health returned {code}: {}",
            body.trim()
        )));
    }
    let (code, body) = get(port, "/props")?;
    if code != 200 {
        return Err(other(format!("/props returned {code}")));
    }
    let props: Value =
        serde_json::from_str(&body).map_err(|e| other(format!("/props is not JSON: {e}")))?;
    parse_props(props)
}

fn parse_props(props: Value) -> Result<ServerInfo> {
    if props["chat_template_caps"]["supports_tool_calls"] == Value::Bool(false) {
        return Err(other(
            "the server's chat template does not support tool calls; start llama-server with --jinja",
        ));
    }
    let n_ctx = props["default_generation_settings"]["n_ctx"]
        .as_u64()
        .ok_or_else(|| other("/props has no default_generation_settings.n_ctx"))?;
    Ok(ServerInfo { n_ctx, props })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn props_give_context_window() {
        let p = json!({"default_generation_settings": {"n_ctx": 65536},
                       "chat_template_caps": {"supports_tool_calls": true}});
        assert_eq!(parse_props(p).unwrap().n_ctx, 65536);
    }

    #[test]
    fn props_without_tool_calls_refuse() {
        let p = json!({"default_generation_settings": {"n_ctx": 1},
                       "chat_template_caps": {"supports_tool_calls": false}});
        let err = parse_props(p).unwrap_err().to_string();
        assert!(err.contains("--jinja"), "{err}");
    }

    #[test]
    fn get_reads_status_and_body() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 64];
            while !req.ends_with(b"\r\n\r\n") {
                let n = s.read(&mut buf).unwrap();
                req.extend_from_slice(&buf[..n]);
            }
            s.write_all(
                b"HTTP/1.1 503 Service Unavailable\r\nX: y\r\n\r\n{\"status\":\"loading\"}",
            )
            .unwrap();
        });
        let (code, body) = get(port, "/health").unwrap();
        t.join().unwrap();
        assert_eq!(code, 503);
        assert_eq!(body, "{\"status\":\"loading\"}");
    }
}
