#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! The example extensions (examples/extensions) pass their own tests
//! (Phase 5: every API area has an example), and the script tool example
//! answers JSON on stdin.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn examples() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/extensions")
}

#[test]
fn every_example_passes_its_tests() {
    let runs = psi_ext::testing::run_all(&examples());
    let names: Vec<String> = runs
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "ask-first",
            "changes",
            "fetch-cache",
            "focus",
            "git-guard",
            "keep-generated",
            "llama-box",
            "tally",
            "tidy-results"
        ]
    );
    for (p, r) in &runs {
        assert!(r.passed(), "{}:\n{}", p.display(), r.render(true));
        assert!(!r.cases.is_empty());
    }
}

/// Between them, the examples use every function of the API.
#[test]
fn the_examples_cover_every_api_function() {
    let mut code = String::new();
    for e in std::fs::read_dir(examples()).unwrap().flatten() {
        if let Ok(t) = std::fs::read_to_string(e.path().join("init.lua")) {
            code.push_str(&t);
        }
    }
    let missing: Vec<&str> = psi_ext_api::FUNCTIONS
        .iter()
        .map(|f| f.name)
        .filter(|name| {
            let short = name.rsplit('.').next().unwrap_or(name);
            !code.contains(&format!("{short}(")) && !code.contains(&format!("{short}{{"))
        })
        .collect();
    assert!(missing.is_empty(), "no example uses: {missing:?}");
}

#[test]
fn the_script_tool_example_reads_json_and_prints_the_count() {
    let dir = examples().join("word-count");
    let Some(Ok(psi_ext::Source::Script(t))) = psi_ext::load::source(&dir) else {
        panic!("word-count is a script tool");
    };
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.txt"), "one two three\nfour\n").unwrap();
    let run = |path: &str| {
        let mut c = Command::new(&t.argv()[0])
            .current_dir(d.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin
            .take()
            .unwrap()
            .write_all(format!("{{\"path\": \"{path}\"}}").as_bytes())
            .unwrap();
        let o = c.wait_with_output().unwrap();
        (String::from_utf8(o.stdout).unwrap(), o.status.success())
    };
    assert_eq!(run("a.txt"), ("4\n".into(), true));
    let (out, ok) = run("missing.txt");
    assert!(!ok && out.starts_with("cannot read missing.txt"), "{out}");
}

/// The examples are what authors copy: they lint clean.
#[test]
fn every_example_lints_clean() {
    for e in std::fs::read_dir(examples()).unwrap().flatten() {
        let r = psi_ext::check::check_with(&e.path(), psi_ext::check::Options { luals: false });
        assert_eq!(
            r.errors() + r.warnings(),
            0,
            "{}:\n{}",
            e.path().display(),
            r.render()
        );
    }
}
