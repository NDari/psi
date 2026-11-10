use std::process::Command;

fn psi() -> Command {
    Command::new(env!("CARGO_BIN_EXE_psi"))
}

#[test]
fn version_prints_name_and_version() {
    let out = psi().arg("--version").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.trim(), format!("psi {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn broken_user_settings_fail_with_the_path() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".psi")).unwrap();
    std::fs::write(home.path().join(".psi/settings.jsonc"), "{ \"mode\": ").unwrap();
    let out = psi()
        .env("HOME", home.path())
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains(".psi/settings.jsonc"), "{stderr}");
}

#[test]
fn layers_load_and_flag_overrides() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".psi")).unwrap();
    std::fs::write(
        home.path().join(".psi/settings.jsonc"),
        "{ \"network\": \"on\", \"notify\": \"off\" }",
    )
    .unwrap();
    std::fs::create_dir(project.path().join(".psi")).unwrap();
    // A project's file loads without a trust step, minus what would
    // loosen safety.
    std::fs::write(
        project.path().join(".psi/settings.jsonc"),
        "{ \"isolation\": \"off\", \"post_edit_check\": \"make check\" }",
    )
    .unwrap();
    let out = psi()
        .args(["--network", "off"])
        .env("HOME", home.path())
        .env("PSI_LOG", "debug")
        .current_dir(project.path())
        .output()
        .unwrap();
    // Without a terminal the interactive mode refuses, after loading the settings.
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("the interactive mode needs a terminal"),
        "{stderr}"
    );
    assert!(stderr.contains(r#""isolation":"on""#), "{stderr}");
    assert!(stderr.contains(r#""network":"off""#), "{stderr}");
    assert!(stderr.contains(r#""notify":"off""#), "{stderr}");
    assert!(
        stderr.contains(r#""post_edit_check":"make check""#),
        "{stderr}"
    );
}

const SESSION: &str = r#"{"id":"00000001","parent_id":null,"ts":1,"type":"custom","custom_type":"session_info","data":{"cwd":"/work/project"}}
{"id":"00000002","parent_id":"00000001","ts":2,"type":"message","role":"user","content":[{"type":"text","text":"fix it"}]}
{"id":"00000003","parent_id":"00000002","ts":3,"type":"message","role":"assistant","content":[{"type":"tool_call","id":"c1","name":"read","arguments":{"path":"project/a.py"}}],"stop_reason":"tool_use"}
{"id":"00000004","parent_id":"00000003","ts":4,"type":"tool_result","tool_call_id":"c1","tool_name":"read","content":[{"type":"text","text":"no such file"}],"is_error":true}
"#;

#[test]
fn inspect_shows_steps_and_patterns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, SESSION).unwrap();
    let out = psi().arg("inspect").arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("user: fix it"), "{stdout}");
    assert!(
        stdout.contains("step 1\n  read {\"path\":\"project/a.py\"}\n    ERROR: no such file"),
        "{stdout}"
    );
    assert!(stdout.contains("doubled_cwd_path (step 1)"), "{stdout}");

    let out = psi()
        .args(["inspect", "--json", "--failures"])
        .arg(&path)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["events"].as_array().unwrap().len(), 1);
    assert_eq!(v["events"][0]["kind"], "step");
}

#[test]
fn eval_diff_compares_two_runs() {
    let result = |outcome: &str, passed: u32| {
        serde_json::json!({"task": "t", "trial": 0, "agent": "pi", "outcome": outcome,
            "reason": "", "checks_passed": passed, "checks_total": 2, "steps": 1,
            "tool_calls": 0, "tool_errors": 0, "tokens_in": 0, "tokens_out": 0,
            "step_cap_hit": false, "exit_code": 0, "wall_s": 0})
    };
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    for (dir, r) in [(&a, result("PASS", 2)), (&b, result("MODEL-FAIL", 1))] {
        let t = dir.path().join("t/trial-0");
        std::fs::create_dir_all(&t).unwrap();
        std::fs::write(t.join("result.json"), r.to_string()).unwrap();
    }
    let out = psi()
        .args(["eval", "diff"])
        .arg(a.path())
        .arg(b.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("| t | 0 | PASS 2/2 | MODEL-FAIL 1/2 | changed |"),
        "{stdout}"
    );
}

#[test]
fn eval_refuses_a_directory_that_is_not_evals() {
    let dir = tempfile::tempdir().unwrap();
    let out = psi()
        .args(["eval", "check", "--evals"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("is not the evals directory"), "{stderr}");
}
