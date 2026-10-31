//! Core tools: read, write, edit, bash, and the optional grep, find and ls
//! (psi-plan.md §6). Results say exactly what happened and what to do next.

pub mod bash;
mod edit;
mod files;
mod read;
mod search;
mod text;
mod write;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use psi_ai::ToolSchema;
use psi_safety::Sandbox;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

pub use files::FileTracker;

/// What a tool needs from the session.
#[derive(Debug, Clone)]
pub struct ToolCtx {
    pub cwd: PathBuf,
    /// Files read this session and their content hash at the time (D-24).
    pub files: Arc<Mutex<FileTracker>>,
    /// Results are cut to this many characters (§6: a share of the window).
    pub max_result_chars: usize,
    /// bash's `TMPDIR`, kept for the session.
    pub tmpdir: PathBuf,
    pub cancel: CancellationToken,
    /// The boundary bash runs inside; `None` runs it directly (§8.4).
    pub sandbox: Option<Arc<Sandbox>>,
}

impl ToolCtx {
    pub fn new(cwd: &Path, tmpdir: &Path, max_result_chars: usize) -> Self {
        Self {
            cwd: cwd.to_owned(),
            files: Arc::new(Mutex::new(FileTracker::default())),
            max_result_chars,
            tmpdir: tmpdir.to_owned(),
            cancel: CancellationToken::new(),
            sandbox: None,
        }
    }

    pub(crate) fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_owned()
        } else {
            self.cwd.join(p)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Read,
    Write,
    Edit,
    Bash,
    Grep,
    Find,
    Ls,
    // Extension and script tools are psi_agent::ext::Callable::Ext.
}

/// The tools a session offers: pi's four, plus grep/find/ls when enabled (D-17).
pub fn core_tools(grep_find_ls: bool) -> Vec<Tool> {
    let mut t = vec![Tool::Read, Tool::Write, Tool::Edit, Tool::Bash];
    if grep_find_ls {
        t.extend([Tool::Grep, Tool::Find, Tool::Ls]);
    }
    t
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Read => "read",
            Tool::Write => "write",
            Tool::Edit => "edit",
            Tool::Bash => "bash",
            Tool::Grep => "grep",
            Tool::Find => "find",
            Tool::Ls => "ls",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [
            Tool::Read,
            Tool::Write,
            Tool::Edit,
            Tool::Bash,
            Tool::Grep,
            Tool::Find,
            Tool::Ls,
        ]
        .into_iter()
        .find(|t| t.name() == name)
    }

    pub fn schema(self) -> ToolSchema {
        match self {
            Tool::Read => read::schema(),
            Tool::Write => write::schema(),
            Tool::Edit => edit::schema(),
            Tool::Bash => bash::schema(),
            Tool::Grep => search::grep_schema(),
            Tool::Find => search::find_schema(),
            Tool::Ls => search::ls_schema(),
        }
    }

    /// Read-only calls in one response may run in parallel (§5).
    pub fn read_only(self) -> bool {
        matches!(self, Tool::Read | Tool::Grep | Tool::Find | Tool::Ls)
    }

    /// The file a mutating call changes, so calls on one path run in order.
    pub fn target(self, args: &Map<String, Value>, ctx: &ToolCtx) -> Option<PathBuf> {
        match self {
            Tool::Write | Tool::Edit => args["path"].as_str().map(|p| ctx.resolve(p)),
            _ => None,
        }
    }

    /// Runs a call whose arguments passed the shape-fix pipeline.
    pub async fn run(self, args: &Map<String, Value>, ctx: &ToolCtx) -> ToolOutput {
        if ctx.cancel.is_cancelled() {
            return ToolOutput::err("Aborted before it ran.");
        }
        let f: fn(&Map<String, Value>, &ToolCtx) -> ToolOutput = match self {
            Tool::Bash => return bash::run(args.clone(), ctx, false).await,
            Tool::Read => read::run,
            Tool::Write => write::run,
            Tool::Edit => edit::run,
            Tool::Grep => search::grep,
            Tool::Find => search::find,
            Tool::Ls => search::ls,
        };
        let (args, ctx) = (args.clone(), ctx.clone());
        tokio::task::spawn_blocking(move || f(&args, &ctx))
            .await
            .unwrap_or_else(|e| ToolOutput::err(format!("The tool crashed: {e}")))
    }
}

pub(crate) fn str_arg<'a>(args: &'a Map<String, Value>, k: &str) -> &'a str {
    args.get(k).and_then(Value::as_str).unwrap_or("")
}

pub(crate) fn int_arg(args: &Map<String, Value>, k: &str) -> Option<i64> {
    args.get(k).and_then(Value::as_i64)
}
