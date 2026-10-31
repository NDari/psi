#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! The eval pipeline end to end without a GPU (D-32): `psi eval run` with
//! the replay server as the model, scripted to solve fix_invoice_rounding by
//! writing the reference solution. Needs podman; run with `just eval-smoke`.

use std::path::Path;
use std::process::Command;

use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use serde_json::{Value, json};

fn script() -> Vec<Reply> {
    let reference = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../evals/code/fix_invoice_rounding/reference/billing");
    let files = ["money.py", "tax.py"];
    let read = Reply::Chat(ScriptedReply {
        tool_calls: files
            .iter()
            .map(|f| {
                (
                    "read".to_owned(),
                    json!({"path": format!("billing/{f}")}).to_string(),
                )
            })
            .collect(),
        ..Default::default()
    });
    let write = Reply::Chat(ScriptedReply {
        tool_calls: files
            .iter()
            .map(|f| {
                let content = std::fs::read_to_string(reference.join(f)).unwrap();
                (
                    "write".to_owned(),
                    json!({"path": format!("billing/{f}"), "content": content}).to_string(),
                )
            })
            .collect(),
        ..Default::default()
    });
    vec![
        read,
        write,
        Reply::Chat(ScriptedReply::text("Fixed parse_amount and line_vat.")),
    ]
}

/// The static build `just eval-smoke` makes: the host's glibc can be newer
/// than the image's.
fn eval_binary() -> String {
    std::env::var("PSI_EVAL_BINARY")
        .expect("set PSI_EVAL_BINARY to the static psi build; run with just eval-smoke")
}

fn run(agent: &str) {
    let server = ReplayServer::start(script()).unwrap();
    server.set_n_ctx(16384);
    let results = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_psi"))
        .args([
            "eval",
            "run",
            "--agent",
            agent,
            "--tasks",
            "fix_invoice_rounding",
            "--timeout",
            "120",
            "--psi-binary",
            &eval_binary(),
        ])
        .arg("--port")
        .arg(server.port().to_string())
        .arg("--evals")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals"))
        .arg("--results")
        .arg(results.path().join("run"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{agent}: {stderr}\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let r: Value = serde_json::from_str(
        &std::fs::read_to_string(
            results
                .path()
                .join("run/fix_invoice_rounding/trial-0/result.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(r["outcome"], "PASS", "{agent}: {r}");
    assert_eq!(r["steps"], 3, "{agent}: {r}");
    assert_eq!(server.remaining(), 0, "{agent}");
    let session = results
        .path()
        .join("run/fix_invoice_rounding/trial-0/session.jsonl");
    assert!(std::fs::metadata(session).unwrap().len() > 0, "{agent}");
}

#[test]
#[ignore = "needs podman; run with just eval-smoke"]
fn psi_trial_passes_through_the_whole_pipeline() {
    run("psi");
}

#[test]
#[ignore = "needs podman; run with just eval-smoke"]
fn pi_trial_passes_through_the_whole_pipeline() {
    run("pi");
}
