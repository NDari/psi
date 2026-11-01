//! A fake llama-server that plays back recorded or scripted responses
//! (D-32), so tests drive the real client, loop and tools in seconds.
//!
//! Recorded responses are written byte for byte; scripted ones are rendered
//! the way llama-server streams (see tests/fixtures/llama-server).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One answer to a chat request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    /// A recorded HTTP response, written as is.
    Raw {
        #[serde(with = "lossy_bytes")]
        bytes: Vec<u8>,
    },
    /// A scripted answer, streamed like llama-server.
    Chat(ScriptedReply),
    /// An error response.
    Error { status: u16, body: Value },
    /// A stream that starts with `text` and then sends nothing more for
    /// half a minute (a stalled server; tests abort it).
    Hang { text: String },
}

impl Reply {
    /// A 200 stream with `body` as its SSE events, closed after them.
    fn stream(body: &str) -> Self {
        Reply::Raw {
            bytes: stream_bytes(body),
        }
    }

    /// An answer that starts with `text` and stops: no finish reason, no
    /// `[DONE]` (a server that died or a dropped connection).
    pub fn cut(text: &str) -> Self {
        Reply::Raw {
            bytes: cut_bytes(text),
        }
    }

    /// A stream that sends `{"error": ...}` after a little text.
    pub fn stream_error(message: &str) -> Self {
        Reply::stream(&format!(
            "data: {}\n\ndata: {}\n\n",
            json!({"choices": [{"finish_reason": null, "index": 0, "delta": {"content": "Let me"}}]}),
            json!({"error": {"code": 500, "message": message, "type": "server_error"}})
        ))
    }

    pub fn hang(text: &str) -> Self {
        Reply::Hang { text: text.into() }
    }

    /// A stream whose data isn't JSON.
    pub fn garbage() -> Self {
        Reply::stream("data: <html>Bad Gateway</html>\n\n")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScriptedReply {
    #[serde(default)]
    pub text: String,
    /// (tool name, arguments as JSON text)
    #[serde(default)]
    pub tool_calls: Vec<(String, String)>,
    /// "stop", "tool_calls" or "length"; derived from the calls when unset.
    #[serde(default)]
    pub finish: Option<String>,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

impl ScriptedReply {
    pub fn text(t: &str) -> Self {
        Self {
            text: t.into(),
            ..Self::default()
        }
    }

    pub fn call(name: &str, args: Value) -> Self {
        Self {
            tool_calls: vec![(name.into(), args.to_string())],
            ..Self::default()
        }
    }
}

mod lossy_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(b: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&String::from_utf8_lossy(b))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        Ok(String::deserialize(d)?.into_bytes())
    }
}

impl Reply {
    /// A recorded `*.response.http` file.
    pub fn from_http_file(path: &Path) -> std::io::Result<Self> {
        Ok(Reply::Raw {
            bytes: std::fs::read(path)?,
        })
    }
}

/// A request the server received.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    /// Header names in lowercase, with their values.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

#[derive(Debug)]
struct State {
    replies: VecDeque<Reply>,
    requests: Vec<Recorded>,
    props: Value,
}

pub struct ReplayServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
    handle: Option<JoinHandle<()>>,
}

fn lock(s: &Mutex<State>) -> MutexGuard<'_, State> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

impl ReplayServer {
    /// Starts on a free port on 127.0.0.1.
    pub fn start(replies: Vec<Reply>) -> std::io::Result<Self> {
        Self::start_on(0, replies)
    }

    pub fn start_on(port: u16, replies: Vec<Reply>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let addr = listener.local_addr()?;
        let state = Arc::new(Mutex::new(State {
            replies: replies.into(),
            requests: Vec::new(),
            props: json!({
                "default_generation_settings": {"n_ctx": 32768},
                "chat_template_caps": {"supports_tool_calls": true},
                "model_path": "replay",
                "build_info": "replay",
            }),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        let handle = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if sp.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(conn) = conn else { continue };
                let st = st.clone();
                std::thread::spawn(move || {
                    if let Err(e) = serve(conn, &st) {
                        tracing::debug!("replay server: {e}");
                    }
                });
            }
        });
        Ok(Self {
            addr,
            stop,
            state,
            handle: Some(handle),
        })
    }

    /// Sets the context window `/props` reports.
    pub fn set_n_ctx(&self, n_ctx: u64) {
        lock(&self.state).props["default_generation_settings"]["n_ctx"] = json!(n_ctx);
    }

    pub fn set_props(&self, props: Value) {
        lock(&self.state).props = props;
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// `http://127.0.0.1:<port>/v1`, as in models.jsonc.
    pub fn base_url(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    pub fn requests(&self) -> Vec<Recorded> {
        lock(&self.state).requests.clone()
    }

    /// The chat requests received, parsed.
    pub fn chat_requests(&self) -> Vec<Value> {
        self.requests()
            .iter()
            .filter(|r| r.path.ends_with("/chat/completions"))
            .map(Recorded::json)
            .collect()
    }

    pub fn remaining(&self) -> usize {
        lock(&self.state).replies.len()
    }

    /// Counts the chat requests received so far, from any thread (a test
    /// waiting for a request to be in flight).
    pub fn chat_counter(&self) -> impl Fn() -> usize + Send + Sync + 'static {
        let state = self.state.clone();
        move || {
            lock(&state)
                .requests
                .iter()
                .filter(|r| r.path.ends_with("/chat/completions"))
                .count()
        }
    }
}

impl Drop for ReplayServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so it sees the flag.
        let _ = TcpStream::connect(self.addr);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn serve(mut conn: TcpStream, state: &Mutex<State>) -> std::io::Result<()> {
    let mut reader = BufReader::new(conn.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("").to_owned();
    let mut len = 0usize;
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        if h == "\r\n" || h == "\n" || h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
        }
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body)?;
    let response = {
        let mut st = lock(state);
        st.requests.push(Recorded {
            method: method.clone(),
            path: path.clone(),
            headers,
            body: body.clone(),
        });
        match (method.as_str(), path.as_str()) {
            ("GET", "/health") => http(200, "application/json", br#"{"status":"ok"}"#),
            ("GET", "/props") => http(200, "application/json", st.props.to_string().as_bytes()),
            ("POST", "/tokenize") => {
                let v: Value = serde_json::from_slice(&body).unwrap_or_default();
                let n = v["content"].as_str().map_or(0, |c| c.len().div_ceil(4));
                http(200, "application/json", json!({"tokens": vec![0; n]}).to_string().as_bytes())
            }
            ("POST", p) if p.ends_with("/chat/completions") && !history_parses(&body) => http(
                500,
                "application/json",
                br#"{"error":{"code":500,"message":"Failed to parse tool call arguments as JSON: replay server","type":"server_error"}}"#,
            ),
            ("POST", p) if p.ends_with("/chat/completions") => match st.replies.pop_front() {
                Some(Reply::Raw { bytes }) => bytes,
                Some(Reply::Chat(r)) => http(200, "text/event-stream", render_sse(&r).as_bytes()),
                Some(Reply::Error { status, body }) => {
                    http(status, "application/json", body.to_string().as_bytes())
                }
                Some(Reply::Hang { text }) => {
                    drop(st);
                    conn.write_all(&cut_bytes(&text))?;
                    conn.flush()?;
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    return Ok(());
                }
                None => http(
                    500,
                    "application/json",
                    br#"{"error":{"code":500,"message":"replay server: no responses left","type":"server_error"}}"#,
                ),
            },
            _ => http(404, "application/json", br#"{"error":{"code":404,"message":"not found"}}"#),
        }
    };
    conn.write_all(&response)?;
    conn.flush()
}

/// llama-server parses the arguments of every past tool call in a request
/// and answers 500 when one is not JSON (recorded in
/// tests/fixtures/llama-server/gemma4-12b-qat/tool_call_parse_error.response.http).
fn history_parses(body: &[u8]) -> bool {
    let v: Value = serde_json::from_slice(body).unwrap_or_default();
    v["messages"].as_array().into_iter().flatten().all(|m| {
        m["tool_calls"].as_array().into_iter().flatten().all(|c| {
            c["function"]["arguments"]
                .as_str()
                .is_none_or(|a| serde_json::from_str::<Value>(a).is_ok())
        })
    })
}

fn stream_bytes(body: &str) -> Vec<u8> {
    let mut bytes = b"HTTP/1.1 200 OK\r\nServer: llama.cpp\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n".to_vec();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

fn cut_bytes(text: &str) -> Vec<u8> {
    let chunk = |delta: Value| {
        json!({"choices": [{"finish_reason": null, "index": 0, "delta": delta}],
               "object": "chat.completion.chunk"})
    };
    stream_bytes(&format!(
        "data: {}\n\ndata: {}\n\n",
        chunk(json!({"role": "assistant", "content": null})),
        chunk(json!({"content": text}))
    ))
}

fn http(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let mut out = format!(
        "HTTP/1.1 {status} {reason}\r\nServer: llama.cpp\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

/// Renders a scripted reply in llama-server's chunk format.
pub fn render_sse(r: &ScriptedReply) -> String {
    let chunk = |choices: Value| {
        json!({"choices": choices, "created": 0, "id": "chatcmpl-replay", "model": "local",
               "system_fingerprint": "replay", "object": "chat.completion.chunk"})
    };
    let mut events = vec![chunk(json!([{"finish_reason": null, "index": 0,
        "delta": {"role": "assistant", "content": null}}]))];
    if !r.text.is_empty() {
        events.push(chunk(
            json!([{"finish_reason": null, "index": 0, "delta": {"content": r.text}}]),
        ));
    }
    for (i, (name, args)) in r.tool_calls.iter().enumerate() {
        events.push(chunk(
            json!([{"finish_reason": null, "index": 0, "delta": {"tool_calls": [{
            "index": i, "id": format!("call_{i}"), "type": "function",
            "function": {"name": name, "arguments": args}}]}}]),
        ));
    }
    let finish = r.finish.clone().unwrap_or_else(|| {
        if r.tool_calls.is_empty() {
            "stop"
        } else {
            "tool_calls"
        }
        .into()
    });
    events.push(chunk(
        json!([{"finish_reason": finish, "index": 0, "delta": {}}]),
    ));
    let mut usage = chunk(json!([]));
    usage["usage"] = json!({"completion_tokens": r.completion_tokens, "prompt_tokens": r.prompt_tokens,
        "total_tokens": r.prompt_tokens + r.completion_tokens, "prompt_tokens_details": {"cached_tokens": 0}});
    events.push(usage);
    let mut out = String::new();
    for e in events {
        out.push_str("data: ");
        out.push_str(&e.to_string());
        out.push_str("\n\n");
    }
    out.push_str("data: [DONE]\n\n");
    out
}
