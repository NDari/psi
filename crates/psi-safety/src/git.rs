//! Hardened host git for Psi's own git calls (psi-plan.md §7.3).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A `git` command for Psi's own calls on the host (not inside the sandbox).
/// Bash can write the workspace's `.git/config`, so these never run its hooks
/// or fsmonitor: no system config, no inherited `GIT_*` variables, stdin null.
pub fn host_git() -> Command {
    let mut cmd = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd.args([
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
    ])
    .env("GIT_CONFIG_NOSYSTEM", "1")
    .env("GIT_TERMINAL_PROMPT", "0")
    .env("LC_ALL", "C")
    .stdin(Stdio::null());
    cmd
}

/// Whether a `git` binary can be run.
pub fn git_available() -> bool {
    host_git()
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The project root: the repository's top level when `cwd` is inside one,
/// else `cwd` itself. Canonicalized when possible.
pub fn project_root(cwd: &Path) -> PathBuf {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    rev_parse(&cwd, &["--show-toplevel"])
        .into_iter()
        .next()
        .and_then(|top| top.canonicalize().ok())
        .unwrap_or(cwd)
}

/// Git directories the sandbox must also make writable: the repository's git
/// dir and common dir when they lie outside `root` (linked worktrees,
/// submodules whose git dir is in the superproject). Empty if not a repo.
pub fn git_dirs_outside(root: &Path) -> Vec<PathBuf> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in rev_parse(&root, &["--absolute-git-dir", "--git-common-dir"]) {
        // --git-common-dir may be relative to the working directory.
        let dir = root.join(dir);
        let dir = dir.canonicalize().unwrap_or(dir);
        if !dir.starts_with(&root) && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// `git rev-parse <args>` in `dir`, one path per output line; empty on failure.
fn rev_parse(dir: &Path, args: &[&str]) -> Vec<PathBuf> {
    let out = host_git()
        .arg("rev-parse")
        .args(args)
        .current_dir(dir)
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = host_git()
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn repo(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        git_in(dir, &["init", "-q"]);
        std::fs::write(dir.join("f"), "x").unwrap();
        git_in(dir, &["add", "f"]);
        git_in(dir, &["commit", "-q", "-m", "init"]);
    }

    #[test]
    fn project_root_finds_top_level_or_falls_back() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let main = base.join("main");
        repo(&main);
        std::fs::create_dir(main.join("sub")).unwrap();
        assert_eq!(project_root(&main.join("sub")), main);
        let plain = base.join("plain");
        std::fs::create_dir(&plain).unwrap();
        assert_eq!(project_root(&plain), plain);
        assert!(git_available());
    }

    #[test]
    fn linked_worktree_has_git_dirs_outside() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let main = base.join("main");
        repo(&main);
        assert!(git_dirs_outside(&main).is_empty());
        assert!(git_dirs_outside(&base).is_empty());

        git_in(&main, &["worktree", "add", "-q", "-b", "wt", "../wt"]);
        let dirs = git_dirs_outside(&base.join("wt"));
        assert_eq!(
            dirs,
            vec![main.join(".git/worktrees/wt"), main.join(".git")]
        );
    }
}
