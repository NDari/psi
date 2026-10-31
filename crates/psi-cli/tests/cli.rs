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
        "{ \"mode\": \"strict\" }",
    )
    .unwrap();
    std::fs::create_dir(project.path().join(".psi")).unwrap();
    std::fs::write(
        project.path().join(".psi/settings.jsonc"),
        "{ \"network\": \"off\" }",
    )
    .unwrap();
    let out = psi()
        .args(["--mode", "auto"])
        .env("HOME", home.path())
        .env("PSI_LOG", "debug")
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains(r#""mode":"auto""#), "{stderr}");
    assert!(stderr.contains(r#""network":"off""#), "{stderr}");
}
