//! `psi.exec` and `psi.http` for extensions (psi-plan.md §9.4, D-45): a
//! command goes through the same always-ask rules and boundary as bash,
//! and a request honors the `network` setting. Both stop on Esc.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, Weak};

use futures_util::future::BoxFuture;
use psi_ext::{ExecCall, ExecOutput, ExtManager, HttpCall, HttpOutput};
use psi_safety::policy::{Decision, Policy};
use psi_tools::ToolCtx;
use psi_tools::bash::{Capture, capture};
use serde_json::json;

use crate::approval::{Approval, ApprovalRequest, Approver};

pub struct ExtServices {
    pub ctx: ToolCtx,
    pub policy: Policy,
    pub approver: Arc<dyn Approver>,
    /// The session's grants when the run started (`--allow`, `s` answers).
    pub grants: HashSet<String>,
    /// `s` answers to extension commands, kept across runs.
    pub ext_grants: Arc<Mutex<HashSet<String>>>,
    pub ext: Weak<ExtManager>,
}

/// One shell word, quoted only when it needs to be.
fn quote(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:=@%+,".contains(c));
    if plain {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// The command as the always-ask rules see it: a shell's `-c` script as
/// written, anything else as one quoted command line.
pub fn shown_command(argv: &[String]) -> String {
    let shell = argv.first().map(|p| p.rsplit('/').next().unwrap_or(p));
    if matches!(shell, Some("sh" | "bash" | "zsh" | "dash"))
        && argv.get(1).map(String::as_str) == Some("-c")
        && let Some(script) = argv.get(2)
    {
        return script.clone();
    }
    argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
}

impl ExtServices {
    fn granted(&self, keys: &[String]) -> bool {
        let ext = self
            .ext_grants
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        keys.iter()
            .all(|k| self.grants.contains(k) || ext.contains(k))
    }
}

impl psi_ext::Services for ExtServices {
    fn exec(&self, name: String, call: ExecCall) -> BoxFuture<'static, Result<ExecOutput, String>> {
        let shown = shown_command(&call.argv);
        let decision = self.policy.bash(&shown);
        let granted = match &decision {
            Decision::Ask { keys, .. } => self.granted(keys),
            _ => false,
        };
        let (ctx, approver, ext_grants) = (
            self.ctx.clone(),
            self.approver.clone(),
            self.ext_grants.clone(),
        );
        let outside = self.ext.upgrade().is_some_and(|m| m.is_unsandboxed(&name));
        Box::pin(async move {
            match decision {
                Decision::Allow => {}
                Decision::Refuse(m) => return Err(m),
                Decision::Ask { .. } if granted => {}
                Decision::Ask { reason, keys } => {
                    let req = ApprovalRequest {
                        tool_call_id: String::new(),
                        tool: "bash".into(),
                        args: json!({ "command": shown }),
                        intent: Some(format!("the {name} extension runs it")),
                        preceding_text: String::new(),
                        reason: reason.clone(),
                        keys: keys.clone(),
                        agent: None,
                    };
                    match approver.approve(req).await {
                        Approval::Once => {}
                        Approval::Session => {
                            if let Ok(mut g) = ext_grants.lock() {
                                g.extend(keys);
                            }
                        }
                        _ => return Err(format!("not run: {reason}, and it was denied")),
                    }
                }
            }
            let c = Capture {
                cwd: call
                    .cwd
                    .map_or_else(|| ctx.cwd.clone(), |d| ctx.cwd.join(d)),
                env: call.env,
                stdin: call.stdin,
                timeout: call.timeout,
                max_bytes: call.max_bytes,
            };
            let r = capture(&call.argv, c, &ctx, outside).await?;
            Ok(ExecOutput {
                code: r.code,
                stdout: r.stdout,
                stderr: r.stderr,
                killed: r.killed,
                truncated: r.truncated,
            })
        })
    }

    fn http(&self, _: String, call: HttpCall) -> BoxFuture<'static, Result<HttpOutput, String>> {
        let network = self.ctx.sandbox.as_ref().is_none_or(|s| s.config().network);
        let cancel = self.ctx.cancel.clone();
        Box::pin(async move {
            if !network {
                return Err(format!(
                    "the network is off (the `network` setting), so {} can't be reached",
                    call.url
                ));
            }
            tokio::select! {
                r = request(call) => r,
                () = cancel.cancelled() => Err("aborted".into()),
            }
        })
    }
}

async fn request(call: HttpCall) -> Result<HttpOutput, String> {
    let mut url = reqwest::Url::parse(&call.url).map_err(|e| format!("{}: {e}", call.url))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("{}: only http and https URLs", call.url));
    }
    let method = reqwest::Method::from_bytes(call.method.as_bytes())
        .map_err(|_| format!("{:?} is not an HTTP method", call.method))?;
    let client = reqwest::Client::builder()
        .timeout(call.timeout)
        .build()
        .map_err(|e| e.to_string())?;
    if !call.query.is_empty() {
        url.query_pairs_mut().extend_pairs(&call.query);
    }
    let mut req = client.request(method, url);
    for (k, v) in &call.headers {
        req = req.header(k, v);
    }
    if let Some(b) = call.body {
        req = req.body(b);
    }
    let fail = |e: reqwest::Error| {
        if e.is_timeout() {
            format!(
                "{}: timed out after {} ms",
                call.url,
                call.timeout.as_millis()
            )
        } else {
            format!("{}: {e}", call.url)
        }
    };
    let mut resp = req.send().await.map_err(fail)?;
    let mut headers: Vec<(String, String)> = Vec::new();
    for (k, v) in resp.headers() {
        let v = String::from_utf8_lossy(v.as_bytes()).into_owned();
        match headers.iter_mut().find(|(n, _)| n == k.as_str()) {
            Some((_, old)) => {
                old.push_str(", ");
                old.push_str(&v);
            }
            None => headers.push((k.as_str().to_owned(), v)),
        }
    }
    let status = resp.status().as_u16();
    let mut body = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = resp.chunk().await.map_err(fail)? {
        let room = call.max_bytes.saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if chunk.len() > room {
            truncated = true;
            break;
        }
    }
    Ok(HttpOutput {
        status,
        headers,
        body,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    fn services(root: &std::path::Path) -> ExtServices {
        let ctx = ToolCtx::new(root, &root.join("tmp"), 1000);
        ExtServices {
            policy: Policy::new(
                psi_safety::policy::Mode::Auto,
                psi_safety::always_ask::Rules::shipped(),
                root,
                None,
                &root.join("tmp"),
            ),
            ctx,
            approver: Arc::new(crate::approval::Headless),
            grants: HashSet::new(),
            ext_grants: Arc::default(),
            ext: Weak::new(),
        }
    }

    #[tokio::test]
    async fn http_requests_reach_the_server_with_query_and_cap() {
        use psi_ext::Services as _;
        let server = psi_ai::replay::ReplayServer::start(vec![]).unwrap();
        let d = tempfile::tempdir().unwrap();
        let s = services(d.path());
        let url = server.base_url().replace("/v1", "/props");
        let mut call = psi_ext::host::http_get_call(url.clone());
        let r = s.http("x".into(), call.clone()).await.unwrap();
        assert_eq!(r.status, 200);
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k == "content-type" && v == "application/json")
        );
        let props: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert!(props["default_generation_settings"].is_object());
        // Any status is a response.
        let mut q = call.clone();
        q.query = vec![("a".into(), "b c".into())];
        let r = s.http("x".into(), q).await.unwrap();
        assert_eq!(r.status, 404);
        assert_eq!(server.requests().last().unwrap().path, "/props?a=b+c");
        call.max_bytes = 5;
        let r = s.http("x".into(), call).await.unwrap();
        assert_eq!((r.body.len(), r.truncated), (5, true));
        let e = s
            .http("x".into(), psi_ext::host::http_get_call("ftp://x/".into()))
            .await
            .unwrap_err();
        assert_eq!(e, "ftp://x/: only http and https URLs");
    }

    #[tokio::test]
    async fn headless_runs_refuse_commands_that_ask() {
        use psi_ext::Services as _;
        let d = tempfile::tempdir().unwrap();
        let s = services(d.path());
        let call = |argv: &[&str]| psi_ext::ExecCall {
            argv: super::tests::argv(argv),
            cwd: None,
            env: vec![],
            stdin: None,
            timeout: std::time::Duration::from_secs(5),
            max_bytes: 100,
        };
        let r = s.exec("x".into(), call(&["echo", "hi"])).await.unwrap();
        assert_eq!((r.code, r.stdout.as_slice()), (Some(0), &b"hi\n"[..]));
        let e = s
            .exec("x".into(), call(&["git", "clean", "-fd"]))
            .await
            .unwrap_err();
        assert!(
            e.starts_with("not run: `git clean` is on the always-ask list"),
            "{e}"
        );
    }

    #[test]
    fn commands_are_shown_as_bash_would_read_them() {
        assert_eq!(shown_command(&argv(&["git", "status"])), "git status");
        assert_eq!(
            shown_command(&argv(&["rm", "-rf", "my dir", "it's"])),
            r"rm -rf 'my dir' 'it'\''s'"
        );
        assert_eq!(
            shown_command(&argv(&["/bin/sh", "-c", "git push -f && echo ok"])),
            "git push -f && echo ok"
        );
        assert_eq!(shown_command(&argv(&["echo", ""])), "echo ''");
    }
}
