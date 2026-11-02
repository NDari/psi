//! Spotting a write the boundary blocked (psi-plan.md §8.5, D-23), so the
//! harness can offer the user a rerun instead of the model trying variants.

use std::path::{Component, Path, PathBuf};

/// The first path a bash output says was blocked: on a line containing
/// "read-only file system" (any case), the last absolute path, resolved
/// through symlinks, when it lies outside every writable directory.
pub fn blocked_path(output: &str, writable: &[PathBuf]) -> Option<PathBuf> {
    blocked_path_with(output, writable, "read-only file system")
}

/// [`blocked_path`] for a backend whose denial says `marker` (lowercase):
/// Seatbelt's "operation not permitted" is also an ordinary error, which
/// the writable check keeps from counting.
pub fn blocked_path_with(output: &str, writable: &[PathBuf], marker: &str) -> Option<PathBuf> {
    let writable: Vec<PathBuf> = writable.iter().map(|d| resolve_path(d)).collect();
    output
        .lines()
        .filter(|l| l.to_ascii_lowercase().contains(marker))
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

/// Where a write to `p` lands, resolved the way the OS resolves it:
/// symlinks before `..`, so `link/../x` climbs from the link's target, not
/// from `link`'s directory (a lexical `..` let a write through a symlink
/// past the policy, 2026-11-01). The longest existing prefix is resolved by
/// the OS; a dangling symlink is followed to its target; the rest, which
/// doesn't exist yet, is normalized as text.
pub fn resolve_path(p: &Path) -> PathBuf {
    resolve_depth(p, 0)
}

/// Symlinks followed before giving up (Linux's own limit).
const MAX_LINKS: usize = 40;

fn resolve_depth(p: &Path, depth: usize) -> PathBuf {
    let mut existing = p.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let base = loop {
        if let Ok(real) = existing.canonicalize() {
            break real;
        }
        // A dangling symlink: the write creates its target.
        if depth < MAX_LINKS
            && existing
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
            && let Ok(target) = std::fs::read_link(&existing)
        {
            let parent = existing.parent().map(Path::to_path_buf).unwrap_or_default();
            break resolve_depth(&parent.join(target), depth + 1);
        }
        match existing.components().next_back() {
            Some(last) => {
                rest.push(last.as_os_str().to_owned());
                existing.pop();
            }
            None => return lexical(p),
        }
        if existing.as_os_str().is_empty() {
            return lexical(p);
        }
    };
    let mut out = base;
    for r in rest.iter().rev() {
        match Path::new(r).components().next() {
            Some(Component::ParentDir) => {
                out.pop();
            }
            Some(Component::CurDir) | None => {}
            Some(_) => out.push(r),
        }
    }
    out
}

/// `p` with `.` and `..` removed as text (nothing of it exists).
fn lexical(p: &Path) -> PathBuf {
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
    norm
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
            // resolved: /etc and /var are links into /private on macOS
            assert_eq!(got, Some(resolve_path(Path::new(want))), "{out}");
        }
    }

    /// Seatbelt (macOS) denies with EPERM; inside a writable directory
    /// the same text is an ordinary error, not a block.
    #[test]
    fn seatbelt_denials_name_the_path() {
        let m = "operation not permitted";
        let cases = [
            ("touch: /etc/hosts: Operation not permitted", "/etc/hosts"),
            (
                "bash: /opt/x/out.txt: Operation not permitted",
                "/opt/x/out.txt",
            ),
            ("mkdir: /srv/data: Operation not permitted", "/srv/data"),
            (
                "PermissionError: [Errno 1] Operation not permitted: '/usr/local/x'",
                "/usr/local/x",
            ),
        ];
        for (out, want) in cases {
            let got = blocked_path_with(out, &w(), m);
            assert_eq!(got, Some(resolve_path(Path::new(want))), "{out}");
        }
        assert_eq!(
            blocked_path_with("chmod: /work/project/a: Operation not permitted", &w(), m),
            None
        );
        assert_eq!(
            blocked_path_with("touch: /etc/hosts: Read-only file system", &w(), m),
            None
        );
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

    #[test]
    fn dotdot_after_a_symlink_climbs_from_its_target() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let ws = d.path().join("ws");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(home.join(".psi")).unwrap();
        std::fs::create_dir_all(ws.join("x")).unwrap();
        std::os::unix::fs::symlink(home.join(".config"), ws.join("x/cfg")).unwrap();
        // The OS writes this to home/.psi, not ws/x/.psi.
        let p = ws.join("x/cfg/../.psi/settings.jsonc");
        let home = home.canonicalize().unwrap();
        assert_eq!(resolve_path(&p), home.join(".psi/settings.jsonc"));
        // A tail that doesn't exist yet is normalized as text.
        let ws = ws.canonicalize().unwrap();
        assert_eq!(resolve_path(&ws.join("new/../a/./b")), ws.join("a/b"));
    }

    #[test]
    fn a_dangling_symlink_resolves_to_where_the_write_lands() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let outside = d.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(outside.join("new.txt"), ws.join("link")).unwrap();
        assert_eq!(
            resolve_path(&ws.join("link")),
            outside.canonicalize().unwrap().join("new.txt")
        );
        // A symlink loop doesn't hang.
        std::os::unix::fs::symlink(ws.join("b"), ws.join("a")).unwrap();
        std::os::unix::fs::symlink(ws.join("a"), ws.join("b")).unwrap();
        let _ = resolve_path(&ws.join("a/f"));
    }
}
