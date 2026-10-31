//! Spotting a write the boundary blocked (psi-plan.md §8.5, D-23), so the
//! harness can offer the user a rerun instead of the model trying variants.

use std::path::{Component, Path, PathBuf};

/// The first path a bash output says was blocked: on a line containing
/// "read-only file system" (any case), the last absolute path, resolved
/// through symlinks, when it lies outside every writable directory.
pub fn blocked_path(output: &str, writable: &[PathBuf]) -> Option<PathBuf> {
    let writable: Vec<PathBuf> = writable.iter().map(|d| resolve_path(d)).collect();
    output
        .lines()
        .filter(|l| l.to_ascii_lowercase().contains("read-only file system"))
        .filter_map(last_abs_path)
        .map(|p| resolve_path(&p))
        .find(|p| !writable.iter().any(|w| p.starts_with(w)))
}

/// The directory to offer adding for `path`: its nearest existing parent.
pub fn dir_to_add(path: &Path) -> PathBuf {
    let mut d = path.parent().unwrap_or(Path::new("/"));
    while !d.is_dir() {
        match d.parent() {
            Some(p) => d = p,
            None => break,
        }
    }
    d.to_owned()
}

/// What the model is told when the user can't or won't rerun it.
pub fn blocked_message(path: &Path) -> String {
    format!(
        "Blocked: writing {} is outside the workspace. Explain to the user what you needed instead of trying variants.",
        path.display()
    )
}

fn last_abs_path(line: &str) -> Option<PathBuf> {
    let trim: &[char] = &['\'', '"', '`', ':', ',', ';', '(', ')', '[', ']', '‘', '’'];
    line.split_whitespace()
        .filter_map(|w| {
            let w = w.trim_matches(trim);
            // `/x/y: Read-only` leaves the colon on the right; a quoted path
            // inside a word (`'/a/b'`) is trimmed above.
            let start = w.find('/')?;
            let w = &w[start..];
            (w.len() > 1 && !w.starts_with("//")).then(|| PathBuf::from(w.trim_matches(trim)))
        })
        .next_back()
}

/// Lexically normalizes `p`, then resolves symlinks in its longest
/// existing prefix.
pub fn resolve_path(p: &Path) -> PathBuf {
    let mut norm = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                norm.pop();
            }
            Component::CurDir => {}
            c => norm.push(c),
        }
    }
    let mut existing = norm.clone();
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            let mut out = real;
            for r in rest.iter().rev() {
                out.push(r);
            }
            return out;
        }
        match (
            existing.file_name().map(|n| n.to_owned()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_owned();
            }
            _ => return norm,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w() -> Vec<PathBuf> {
        vec!["/work/project".into(), "/tmp".into()]
    }

    #[test]
    fn finds_the_blocked_path_in_common_messages() {
        let cases = [
            (
                "touch: cannot touch '/etc/hosts': Read-only file system",
                "/etc/hosts",
            ),
            (
                "bash: line 1: /opt/x/out.txt: Read-only file system",
                "/opt/x/out.txt",
            ),
            (
                "mkdir: cannot create directory ‘/srv/data’: Read-only file system",
                "/srv/data",
            ),
            (
                "OSError: [Errno 30] Read-only file system: '/usr/lib/python3/x.pyc'",
                "/usr/lib/python3/x.pyc",
            ),
            (
                "cp: cannot create regular file '/var/lib/a': read-only file system",
                "/var/lib/a",
            ),
        ];
        for (out, want) in cases {
            let got = blocked_path(&format!("some output\n{out}\nmore"), &w());
            assert_eq!(got.as_deref(), Some(Path::new(want)), "{out}");
        }
    }

    #[test]
    fn writable_paths_and_other_errors_are_not_blocks() {
        assert_eq!(
            blocked_path(
                "touch: cannot touch '/work/project/a': Read-only file system",
                &w()
            ),
            None
        );
        assert_eq!(
            blocked_path("touch: cannot touch '/etc/hosts': Permission denied", &w()),
            None
        );
        // `..` can't climb out of a writable directory unnoticed.
        assert_eq!(
            blocked_path("bash: /work/project/../x: Read-only file system", &w()).as_deref(),
            Some(Path::new("/work/x"))
        );
    }

    #[test]
    fn symlinks_resolve() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real");
        let ws = d.path().join("ws");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        std::os::unix::fs::symlink(&real, ws.join("link")).unwrap();
        let line = format!(
            "touch: cannot touch '{}/link/f': Read-only file system",
            ws.display()
        );
        let got = blocked_path(&line, std::slice::from_ref(&ws)).unwrap();
        assert_eq!(got, real.canonicalize().unwrap().join("f"));
        assert_eq!(dir_to_add(&got), real.canonicalize().unwrap());
    }
}
