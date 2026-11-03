//! bash: one fresh process per call, in the working directory (§6).
//!
//! stdout and stderr share one pipe, so their order is kept. The command
//! runs in its own process group; on timeout or abort the whole group is
//! killed, and so is anything it left running in the background.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use psi_ai::{Param, ParamKind, ToolSchema};
use serde_json::{Map, Value, json};

use crate::text::{clean, head_tail};
use crate::{ToolCtx, ToolOutput, int_arg, str_arg};

const DEFAULT_TIMEOUT_S: i64 = 300;
const MAX_TIMEOUT_S: i64 = 3600;
/// Output kept in memory: the start and the end.
const KEEP_HEAD: usize = 1 << 20;
const KEEP_TAIL: usize = 4 << 20;

pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        name: "bash",
        description: format!(
            "Run a bash command in the working directory and return its output (stdout and stderr together). Each call is a fresh process: cd does not carry over. Long output keeps its start and end. Default timeout {DEFAULT_TIMEOUT_S} seconds."
        ),
        params: vec![
            Param::new("command", ParamKind::String, "The bash command"),
            Param::new(
                "timeout",
                ParamKind::Integer,
                "Seconds before the command is stopped",
            )
            .optional(),
            Param::new("intent", ParamKind::String, "Optional: one line on why").optional(),
        ],
        example: json!({"command": "python3 -m unittest"}),
    }
}

#[derive(Default)]
struct Buf {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    dropped: usize,
    done: bool,
}

enum End {
    Exited(std::process::ExitStatus),
    TimedOut,
    Aborted,
}

/// Runs a bash call; `outside` skips the sandbox (a rerun the user
/// approved, §8.5).
pub async fn run(args: Map<String, Value>, ctx: &ToolCtx, outside: bool) -> ToolOutput {
    let command = str_arg(&args, "command").to_owned();
    if command.trim().is_empty() {
        return ToolOutput::err("command is empty.");
    }
    let timeout = int_arg(&args, "timeout")
        .unwrap_or(DEFAULT_TIMEOUT_S)
        .clamp(1, MAX_TIMEOUT_S) as u64;
    let argv = vec!["bash".to_owned(), "-c".to_owned(), command];
    exec(&argv, None, timeout, ctx, outside).await
}

/// A script tool (D-22): `argv` inside the same boundary as bash, with the
/// arguments as JSON on stdin; its output is the result.
pub async fn run_script(argv: &[String], args: &Value, ctx: &ToolCtx) -> ToolOutput {
    if argv.is_empty() {
        return ToolOutput::err("The script tool has no command.");
    }
    let input = serde_json::to_vec(args).unwrap_or_default();
    exec(argv, Some(input), DEFAULT_TIMEOUT_S as u64, ctx, false).await
}

async fn exec(
    argv: &[String],
    input: Option<Vec<u8>>,
    timeout: u64,
    ctx: &ToolCtx,
    outside: bool,
) -> ToolOutput {
    if let Err(e) = std::fs::create_dir_all(&ctx.tmpdir) {
        return ToolOutput::err(format!(
            "Cannot create TMPDIR {}: {e}",
            ctx.tmpdir.display()
        ));
    }
    let (reader, writer) = match std::io::pipe() {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(format!("Cannot start bash: {e}")),
    };
    let stderr = match writer.try_clone() {
        Ok(w) => w,
        Err(e) => return ToolOutput::err(format!("Cannot start bash: {e}")),
    };
    let mut cmd = command(argv, &ctx.cwd, ctx, outside);
    let stdin = if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    cmd.stdin(stdin).stdout(writer).stderr(stderr);
    let spawned = cmd.spawn();
    // The parent's copies of the pipe's write end must close, or the
    // reader never sees the end of output.
    drop(cmd);
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return ToolOutput::err(format!("Cannot start {}: {e}", argv[0])),
    };
    if let (Some(bytes), Some(mut w)) = (input, child.stdin.take()) {
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let _ = w.write_all(&bytes).await;
        });
    }
    let pgid = child.id();
    let buf = Arc::new(Mutex::new(Buf::default()));
    let collector = {
        let buf = buf.clone();
        std::thread::spawn(move || collect(reader, &buf))
    };

    let end = tokio::select! {
        s = child.wait() => match s {
            Ok(s) => End::Exited(s),
            Err(e) => return ToolOutput::err(format!("bash failed: {e}")),
        },
        () = tokio::time::sleep(Duration::from_secs(timeout)) => End::TimedOut,
        () = ctx.cancel.cancelled() => End::Aborted,
    };
    if !matches!(end, End::Exited(_)) {
        kill_group(pgid);
        let _ = child.wait().await;
    }
    // Give the reader a moment; a background process holding the pipe is
    // stopped, since every call is a fresh process.
    let mut lingered = false;
    if !wait_done(&buf, Duration::from_millis(200)).await {
        lingered = true;
        kill_group(pgid);
        wait_done(&buf, Duration::from_secs(2)).await;
    }
    drop(collector);

    let (raw, dropped) = {
        let g = buf.lock().unwrap_or_else(|p| p.into_inner());
        let mut v = g.head.clone();
        v.extend(g.tail.iter());
        (v, g.dropped)
    };
    let mut text = clean(&String::from_utf8_lossy(&raw));
    if dropped > 0 {
        let cut = head_cut(&text);
        text.insert_str(
            cut,
            &format!("\n[... {dropped} bytes of output dropped ...]\n"),
        );
    }
    let mut text = head_tail(text.trim_end(), ctx.max_result_chars);
    if text.is_empty() {
        text = "(no output)".into();
    }
    let mut is_error = false;
    match end {
        End::Exited(s) => match (s.code(), signal_of(&s)) {
            (Some(0), _) => {}
            (Some(c), _) => {
                text.push_str(&format!("\n[exit code {c}]"));
                is_error = true;
            }
            (None, sig) => {
                text.push_str(&format!("\n[killed by signal {}]", sig.unwrap_or(0)));
                is_error = true;
            }
        },
        End::TimedOut => {
            text.push_str(&format!(
                "\n[timed out after {timeout} seconds; the command was stopped. Give a longer timeout if it needs more time.]"
            ));
            is_error = true;
        }
        End::Aborted => {
            text.push_str("\n[aborted]");
            is_error = true;
        }
    }
    if lingered {
        text.push_str("\n[processes it left running in the background were stopped]");
    }
    ToolOutput { text, is_error }
}

/// `argv` in `cwd`, inside the boundary unless `outside`, in its own
/// process group, with bash's environment.
fn command(argv: &[String], cwd: &Path, ctx: &ToolCtx, outside: bool) -> tokio::process::Command {
    let sandbox = ctx.sandbox.as_ref().filter(|_| !outside);
    let mut cmd = match sandbox {
        Some(sb) => {
            let (program, args) = sb.command(cwd);
            let mut c = tokio::process::Command::new(program);
            c.args(args).args(argv);
            c
        }
        None => {
            let mut c = tokio::process::Command::new(&argv[0]);
            c.args(&argv[1..]);
            c
        }
    };
    cmd.current_dir(cwd).process_group(0).kill_on_drop(true);
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy();
        if hidden_var(&k) {
            cmd.env_remove(k.as_ref());
        }
    }
    // So nothing waits for input, and error messages can be parsed (§8.4).
    cmd.env("TMPDIR", &ctx.tmpdir)
        .env("LC_MESSAGES", "C")
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("TERM", "dumb");
    cmd
}

/// How an extension's command runs (`psi.exec`, D-45).
#[derive(Debug, Clone)]
pub struct Capture {
    pub cwd: PathBuf,
    /// Added to bash's environment.
    pub env: Vec<(String, String)>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    /// Per stream; the rest is read and dropped.
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captured {
    /// `None` when it was stopped.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Stopped by the timeout, an abort or a signal.
    pub killed: bool,
    pub truncated: bool,
}

/// Runs `argv` with stdout and stderr kept apart, inside the same boundary
/// as bash unless `outside`. The error is why it couldn't start.
pub async fn capture(
    argv: &[String],
    c: Capture,
    ctx: &ToolCtx,
    outside: bool,
) -> Result<Captured, String> {
    if argv.is_empty() {
        return Err("no command".into());
    }
    std::fs::create_dir_all(&ctx.tmpdir)
        .map_err(|e| format!("cannot create TMPDIR {}: {e}", ctx.tmpdir.display()))?;
    let mut cmd = command(argv, &c.cwd, ctx, outside);
    let stdin = if c.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    cmd.stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &c.env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", argv[0]))?;
    if let (Some(bytes), Some(mut w)) = (c.stdin, child.stdin.take()) {
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let _ = w.write_all(&bytes).await;
        });
    }
    let pgid = child.id();
    let out = tokio::spawn(read_capped(child.stdout.take(), c.max_bytes));
    let err = tokio::spawn(read_capped(child.stderr.take(), c.max_bytes));
    let status = tokio::select! {
        s = child.wait() => s.ok(),
        () = tokio::time::sleep(c.timeout) => None,
        () = ctx.cancel.cancelled() => None,
    };
    if status.is_none() {
        kill_group(pgid);
        let _ = child.wait().await;
    }
    let collect = async { (out.await.unwrap_or_default(), err.await.unwrap_or_default()) };
    tokio::pin!(collect);
    // Something it left running in the background may hold the pipes.
    let ((stdout, t1), (stderr, t2)) =
        match tokio::time::timeout(Duration::from_millis(200), &mut collect).await {
            Ok(r) => r,
            Err(_) => {
                kill_group(pgid);
                tokio::time::timeout(Duration::from_secs(2), &mut collect)
                    .await
                    .unwrap_or_default()
            }
        };
    let code = status.and_then(|s| s.code());
    Ok(Captured {
        code,
        stdout,
        stderr,
        killed: code.is_none(),
        truncated: t1 || t2,
    })
}

/// Up to `max` bytes of a pipe, reading on to the end; true when cut.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(r: Option<R>, max: usize) -> (Vec<u8>, bool) {
    use tokio::io::AsyncReadExt;
    let Some(mut r) = r else {
        return (Vec::new(), false);
    };
    let mut out = Vec::new();
    let mut cut = false;
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = max.saturating_sub(out.len());
                out.extend_from_slice(&chunk[..n.min(room)]);
                cut |= n > room;
            }
        }
    }
    (out, cut)
}

/// Where the kept start of the output ends, for the dropped-bytes notice.
fn head_cut(text: &str) -> usize {
    let mut i = KEEP_HEAD.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Variables bash never sees: provider keys, and startup files that would
/// run before every command.
fn hidden_var(name: &str) -> bool {
    name.ends_with("_API_KEY") || name == "BASH_ENV" || name == "ENV"
}

fn collect(mut r: std::io::PipeReader, buf: &Mutex<Buf>) {
    let mut chunk = [0u8; 8192];
    loop {
        let n = match r.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut g = buf.lock().unwrap_or_else(|p| p.into_inner());
        for &byte in &chunk[..n] {
            if g.head.len() < KEEP_HEAD {
                g.head.push(byte);
            } else {
                g.tail.push_back(byte);
                if g.tail.len() > KEEP_TAIL {
                    g.tail.pop_front();
                    g.dropped += 1;
                }
            }
        }
    }
    buf.lock().unwrap_or_else(|p| p.into_inner()).done = true;
}

async fn wait_done(buf: &Mutex<Buf>, max: Duration) -> bool {
    let step = Duration::from_millis(10);
    let mut waited = Duration::ZERO;
    loop {
        if buf.lock().unwrap_or_else(|p| p.into_inner()).done {
            return true;
        }
        if waited >= max {
            return false;
        }
        tokio::time::sleep(step).await;
        waited += step;
    }
}

fn kill_group(pgid: Option<u32>) {
    let Some(pid) = pgid.and_then(|p| rustix::process::Pid::from_raw(p as i32)) else {
        return;
    };
    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
}

fn signal_of(s: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    s.signal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    async fn bash(cmd: &str, timeout: Option<i64>) -> ToolOutput {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), &d.path().join("tmp"), 10_000);
        let mut a = json!({"command": cmd});
        if let Some(t) = timeout {
            a["timeout"] = json!(t);
        }
        run(a.as_object().unwrap().clone(), &c, false).await
    }

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    fn opts(cwd: &Path) -> Capture {
        Capture {
            cwd: cwd.to_owned(),
            env: vec![("PSI_X".into(), "x".into())],
            stdin: None,
            timeout: Duration::from_secs(10),
            max_bytes: 1 << 20,
        }
    }

    #[tokio::test]
    async fn capture_keeps_streams_apart() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), &d.path().join("tmp"), 10_000);
        let sh = |script: &str| argv(&["sh", "-c", script]);
        let r = capture(
            &sh("echo out; echo err >&2; echo $PSI_X; exit 3"),
            opts(d.path()),
            &c,
            false,
        )
        .await
        .unwrap();
        assert_eq!(r.stdout, b"out\nx\n");
        assert_eq!(r.stderr, b"err\n");
        assert_eq!((r.code, r.killed, r.truncated), (Some(3), false, false));

        let mut o = opts(d.path());
        o.stdin = Some(b"a\0b".to_vec());
        o.max_bytes = 2;
        let r = capture(&argv(&["cat"]), o, &c, false).await.unwrap();
        assert_eq!((r.stdout.as_slice(), r.truncated), (&b"a\0"[..], true));

        std::fs::create_dir(d.path().join("sub")).unwrap();
        let r = capture(&argv(&["pwd"]), opts(&d.path().join("sub")), &c, false)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&r.stdout)
                .trim_end()
                .ends_with("/sub")
        );

        let err = capture(&argv(&["no-such-program-psi"]), opts(d.path()), &c, false)
            .await
            .unwrap_err();
        assert!(err.starts_with("cannot start no-such-program-psi"), "{err}");
    }

    #[tokio::test]
    async fn capture_stops_on_timeout_and_abort() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), &d.path().join("tmp"), 10_000);
        let start = Instant::now();
        let mut o = opts(d.path());
        o.timeout = Duration::from_millis(300);
        let r = capture(&argv(&["sh", "-c", "echo hi; sleep 30"]), o, &c, false)
            .await
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(
            (r.code, r.killed, r.stdout.as_slice()),
            (None, true, &b"hi\n"[..])
        );

        let cancel = c.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            cancel.cancel();
        });
        let r = capture(&argv(&["sleep", "30"]), opts(d.path()), &c, false)
            .await
            .unwrap();
        assert!(r.killed && start.elapsed() < Duration::from_secs(8));
    }

    #[tokio::test]
    async fn output_keeps_order_and_exit_code() {
        let out = bash("echo out; echo err >&2; echo out2; exit 3", None).await;
        assert_eq!(out, ToolOutput::err("out\nerr\nout2\n[exit code 3]"));
        assert_eq!(bash("true", None).await, ToolOutput::ok("(no output)"));
    }

    #[tokio::test]
    async fn runs_in_cwd_with_session_tmpdir() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), &d.path().join("t"), 10_000);
        let a = json!({"command": "pwd; echo $TMPDIR; echo ${TERM}"});
        let out = run(a.as_object().unwrap().clone(), &c, false).await;
        // pwd prints the resolved path: /var is /private/var on macOS.
        let want = format!(
            "{}\n{}\ndumb",
            d.path().canonicalize().unwrap().display(),
            d.path().join("t").display()
        );
        assert_eq!(out.text, want);
    }

    #[tokio::test]
    async fn timeout_kills_the_group() {
        let start = Instant::now();
        let out = bash("echo started; sleep 30; echo never", Some(1)).await;
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(out.is_error);
        assert!(
            out.text.starts_with("started\n[timed out after 1 seconds"),
            "{}",
            out.text
        );
    }

    #[tokio::test]
    async fn background_leftovers_are_stopped() {
        let start = Instant::now();
        let out = bash("sleep 30 & echo bg", None).await;
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(!out.is_error);
        assert!(out.text.starts_with("bg"), "{}", out.text);
    }

    #[tokio::test]
    async fn abort_stops_the_command() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), d.path(), 10_000);
        let cancel = c.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            cancel.cancel();
        });
        let out = run(
            json!({"command": "sleep 30"}).as_object().unwrap().clone(),
            &c,
            false,
        )
        .await;
        assert!(out.text.ends_with("[aborted]"), "{}", out.text);
    }

    #[tokio::test]
    async fn long_output_keeps_both_ends() {
        let out = bash("seq 1 100000", None).await;
        assert!(out.text.starts_with("1\n2\n"));
        assert!(out.text.ends_with("100000"));
        assert!(out.text.contains("lines omitted"));
    }

    /// The same process handling under bubblewrap: output, timeouts and
    /// background leftovers.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn sandboxed_calls_end_cleanly() {
        let d = tempfile::tempdir().unwrap();
        let mut c = ToolCtx::new(d.path(), &d.path().join("t"), 10_000);
        let cfg = psi_safety::SandboxConfig {
            root: d.path().to_owned(),
            git_dirs: vec![],
            cache_dirs: vec![],
            tmpdir: d.path().join("t"),
            network: true,
        };
        let psi_safety::Isolation::On(sb) = psi_safety::isolation::choose(true, cfg, None) else {
            panic!("bubblewrap must work for the local check");
        };
        c.sandbox = Some(sb);
        let run1 = |cmd: &str, t: i64| {
            let a = json!({"command": cmd, "timeout": t});
            run(a.as_object().unwrap().clone(), &c, false)
        };
        let out = run1("echo hi > f; cat f; echo $TMPDIR", 10).await;
        assert_eq!(out.text, format!("hi\n{}", d.path().join("t").display()));
        let start = Instant::now();
        let out = run1("sleep 30 & echo bg", 10).await;
        assert!(start.elapsed() < Duration::from_secs(5), "{out:?}");
        let out = run1("echo started; sleep 30", 1).await;
        assert!(start.elapsed() < Duration::from_secs(8), "{out:?}");
        assert!(out.text.contains("[timed out after 1 seconds"), "{out:?}");
        let out = run1("touch /etc/psi-test", 10).await;
        assert!(
            out.is_error && out.text.contains("Read-only file system"),
            "{out:?}"
        );
        // A rerun the user approved runs without the sandbox: the host's
        // /tmp, not a private one.
        let marker = format!("/tmp/psi-test-{}", std::process::id());
        let a = json!({"command": format!("touch {marker}")});
        let out = run(a.as_object().unwrap().clone(), &c, true).await;
        assert!(!out.is_error, "{out:?}");
        assert!(std::path::Path::new(&marker).exists());
        let _ = std::fs::remove_file(&marker);
    }

    #[test]
    fn keys_and_startup_files_are_hidden() {
        assert!(hidden_var("OPENAI_API_KEY"));
        assert!(hidden_var("BASH_ENV"));
        assert!(!hidden_var("PATH"));
    }
}
