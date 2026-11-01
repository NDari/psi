//! The isolation boundary (psi-plan.md §8.4, D-07, D-08, D-30): minimal
//! bubblewrap on Linux. It is good enough to stop a mistaken command from
//! writing outside the workspace, and little more.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock};

/// What bash runs inside.
#[derive(Debug, Clone)]
pub enum Isolation {
    /// Every bash call runs under bubblewrap.
    Bwrap(Arc<Sandbox>),
    /// The user turned isolation off: something outside Psi (an eval
    /// container) is the boundary. Bash runs directly and nothing extra asks.
    Off,
    /// No backend works; bash runs directly and every call asks (§8.1).
    Unavailable(String),
}

impl Isolation {
    pub fn sandbox(&self) -> Option<&Arc<Sandbox>> {
        match self {
            Isolation::Bwrap(s) => Some(s),
            _ => None,
        }
    }

    /// The status line's word for it, when it needs one.
    pub fn status(&self) -> Option<&'static str> {
        match self {
            Isolation::Unavailable(_) => Some("UNISOLATED"),
            _ => None,
        }
    }
}

/// The directories one sandbox makes writable, and whether it has network.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// The workspace root (the project's git top level, or the working directory).
    pub root: PathBuf,
    /// Git directories outside the root: linked worktrees, submodules.
    pub git_dirs: Vec<PathBuf>,
    /// Cache directories, made writable when they exist.
    pub cache_dirs: Vec<PathBuf>,
    /// The session's TMPDIR, bound last so nothing covers it.
    pub tmpdir: PathBuf,
    pub network: bool,
}

#[derive(Debug)]
pub struct Sandbox {
    cfg: SandboxConfig,
    /// Directories the user added for this session (§8.5).
    added: RwLock<Vec<PathBuf>>,
}

/// Why a directory can't be a write root.
pub fn refused_write_root(dir: &Path, home: Option<&Path>) -> Option<&'static str> {
    if dir == Path::new("/") {
        return Some("the file system root");
    }
    if home.is_some_and(|h| dir == h) {
        return Some("your home directory");
    }
    None
}

/// The default cache directories (§8.4).
pub fn default_cache_dirs(home: &Path) -> Vec<PathBuf> {
    [
        ".cache",
        ".cargo/registry",
        ".cargo/git",
        ".npm",
        "go/pkg/mod",
    ]
    .iter()
    .map(|d| home.join(d))
    .collect()
}

impl Sandbox {
    /// Checks the directories; `/` and the home directory are refused as
    /// the root or a cache directory.
    pub fn new(mut cfg: SandboxConfig, home: Option<&Path>) -> Result<Self, String> {
        if let Some(why) = refused_write_root(&cfg.root, home) {
            return Err(format!(
                "the workspace is {why} ({}); bash can't be isolated there. Start psi in a project directory",
                cfg.root.display()
            ));
        }
        cfg.cache_dirs.retain(|d| {
            let refused = refused_write_root(d, home).is_some();
            if refused {
                tracing::warn!(dir = %d.display(), "refused as a cache directory");
            }
            !refused
        });
        Ok(Self {
            cfg,
            added: RwLock::new(Vec::new()),
        })
    }

    pub fn config(&self) -> &SandboxConfig {
        &self.cfg
    }

    /// Makes `dir` writable for the rest of the session.
    pub fn add_dir(&self, dir: &Path, home: Option<&Path>) -> Result<(), String> {
        if let Some(why) = refused_write_root(dir, home) {
            return Err(format!(
                "{} is {why}; it can't be made writable",
                dir.display()
            ));
        }
        if let Ok(mut a) = self.added.write()
            && !a.iter().any(|d| d == dir)
        {
            a.push(dir.to_owned());
        }
        Ok(())
    }

    /// Every directory a command inside may write.
    pub fn writable(&self) -> Vec<PathBuf> {
        let mut v = vec![self.cfg.root.clone()];
        v.extend(self.cfg.git_dirs.iter().cloned());
        v.extend(self.cfg.cache_dirs.iter().cloned());
        if let Ok(a) = self.added.read() {
            v.extend(a.iter().cloned());
        }
        v.push(self.cfg.tmpdir.clone());
        v.push("/tmp".into());
        v.push("/dev".into());
        v
    }

    /// `bwrap <flags> --`, to put before the command (§8.4).
    pub fn args(&self, cwd: &Path) -> Vec<OsString> {
        let mut a: Vec<OsString> = Vec::new();
        let mut push = |flag: &str, paths: &[&Path]| {
            a.push(flag.into());
            for p in paths {
                a.push(p.as_os_str().to_owned());
            }
        };
        push("--ro-bind", &[Path::new("/"), Path::new("/")]);
        push("--dev", &[Path::new("/dev")]);
        push("--proc", &[Path::new("/proc")]);
        // A private /tmp per call.
        push("--tmpfs", &[Path::new("/tmp")]);
        push("--bind", &[&self.cfg.root, &self.cfg.root]);
        for d in &self.cfg.git_dirs {
            push("--bind-try", &[d, d]);
        }
        for d in &self.cfg.cache_dirs {
            push("--bind-try", &[d, d]);
        }
        if let Ok(added) = self.added.read() {
            for d in added.iter() {
                push("--bind-try", &[d, d]);
            }
        }
        // Last, so nothing covers it.
        push("--bind", &[&self.cfg.tmpdir, &self.cfg.tmpdir]);
        if !self.cfg.network {
            push("--unshare-net", &[]);
        }
        push("--unshare-pid", &[]);
        push("--new-session", &[]);
        push("--die-with-parent", &[]);
        push("--chdir", &[cwd]);
        push("--", &[]);
        a
    }

    /// Runs `true` under the real flags once (§8.4). The error says what
    /// failed and the likely fix.
    pub fn check(&self) -> Result<(), String> {
        if let Err(e) = std::fs::create_dir_all(&self.cfg.tmpdir) {
            return Err(format!(
                "cannot create TMPDIR {}: {e}",
                self.cfg.tmpdir.display()
            ));
        }
        let out = Command::new("bwrap")
            .args(self.args(&self.cfg.root))
            .arg("true")
            .stdin(Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr).trim().to_owned();
                let mut msg = format!("bubblewrap failed: {err}");
                if err.contains("ermission") || err.contains("namespace") {
                    msg.push_str(". Unprivileged user namespaces may be restricted (Ubuntu 24.04+: see kernel.apparmor_restrict_unprivileged_userns, or add an AppArmor profile for bwrap)");
                }
                Err(msg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err("bubblewrap (bwrap) is not installed; install it to isolate bash".into())
            }
            Err(e) => Err(format!("cannot run bwrap: {e}")),
        }
    }
}

/// Chooses the boundary at startup: `off` when the user turned it off,
/// else bubblewrap if its check passes, else unavailable, with the reason
/// (never a silent fallback).
pub fn choose(enabled: bool, cfg: SandboxConfig, home: Option<&Path>) -> Isolation {
    if !enabled {
        return Isolation::Off;
    }
    if !cfg!(target_os = "linux") {
        // TODO(phase-7): macOS isolation (G-20).
        return Isolation::Unavailable("isolation is only built for Linux so far".into());
    }
    let sb = match Sandbox::new(cfg, home) {
        Ok(s) => s,
        Err(e) => return Isolation::Unavailable(e),
    };
    match sb.check() {
        Ok(()) => Isolation::Bwrap(Arc::new(sb)),
        Err(e) => Isolation::Unavailable(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn cfg(root: &Path, tmp: &Path, network: bool) -> SandboxConfig {
        SandboxConfig {
            root: root.to_owned(),
            git_dirs: vec![],
            cache_dirs: vec![root.join("missing-cache")],
            tmpdir: tmp.to_owned(),
            network,
        }
    }

    fn sh(sb: &Sandbox, cwd: &Path, script: &str) -> (bool, String) {
        let out = Command::new("bwrap")
            .args(sb.args(cwd))
            .args(["bash", "-c", script])
            .output()
            .unwrap();
        let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.success(), s)
    }

    #[test]
    fn home_and_root_are_refused() {
        let home = Path::new("/home/u");
        assert!(refused_write_root(Path::new("/"), Some(home)).is_some());
        assert!(refused_write_root(home, Some(home)).is_some());
        assert!(refused_write_root(&home.join("p"), Some(home)).is_none());
        let c = SandboxConfig {
            root: home.to_owned(),
            git_dirs: vec![],
            cache_dirs: vec![],
            tmpdir: "/x".into(),
            network: true,
        };
        assert!(Sandbox::new(c, Some(home)).is_err());
        let c = SandboxConfig {
            root: home.join("p"),
            git_dirs: vec![],
            cache_dirs: vec![home.to_owned(), home.join(".cache")],
            tmpdir: "/x".into(),
            network: true,
        };
        let sb = Sandbox::new(c, Some(home)).unwrap();
        assert_eq!(sb.config().cache_dirs, [home.join(".cache")]);
        assert!(sb.add_dir(home, Some(home)).is_err());
    }

    #[test]
    fn tmpdir_is_bound_last() {
        let sb = Sandbox::new(cfg(Path::new("/w"), Path::new("/t"), false), None).unwrap();
        let a: Vec<String> = sb
            .args(Path::new("/w/sub"))
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let t = a.iter().rposition(|x| x == "/t").unwrap();
        assert_eq!(a[t - 1], "/t");
        assert_eq!(a[t - 2], "--bind");
        assert!(
            a.iter()
                .skip(t + 1)
                .all(|x| x.starts_with("--") || x == "/w/sub")
        );
        assert!(a.contains(&"--unshare-net".to_owned()));
        assert_eq!(a.last().unwrap(), "--");
    }

    // The boundary itself, under the real bwrap (Linux, local check D-33).
    /// Not under /tmp, which is private inside the sandbox.
    fn outside_tmp() -> tempfile::TempDir {
        let t = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        tempfile::tempdir_in(t).unwrap()
    }

    #[test]
    fn writes_outside_fail_and_tmp_is_private() {
        let d = outside_tmp();
        let root = d.path().join("root");
        let tmp = d.path().join("tmpdir");
        let outside = d.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let sb = match choose(true, cfg(&root, &tmp, true), None) {
            Isolation::Bwrap(s) => s,
            other => panic!("bubblewrap must work for the local check: {other:?}"),
        };
        let (ok, out) = sh(&sb, &root, &format!("echo x > {}/f", outside.display()));
        assert!(!ok, "{out}");
        assert!(out.contains("Read-only file system"), "{out}");
        assert!(!outside.join("f").exists());
        let (ok, out) = sh(
            &sb,
            &root,
            "echo x > inside && echo y > \"$TMPDIR\"/kept; echo z > /tmp/private",
        );
        assert!(ok, "{out}");
        assert!(root.join("inside").exists());
        // /tmp is private per call: the next call doesn't see the file.
        let (ok, out) = sh(&sb, &root, "test -e /tmp/private && echo seen || echo gone");
        assert!(ok);
        assert_eq!(out.trim(), "gone");
        // A directory added for the session becomes writable.
        sb.add_dir(&outside, None).unwrap();
        let (ok, out) = sh(&sb, &root, &format!("echo x > {}/f", outside.display()));
        assert!(ok, "{out}");
    }

    #[test]
    fn network_off_blocks_traffic() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let Isolation::Bwrap(sb) = choose(true, cfg(&root, &d.path().join("t"), false), None)
        else {
            panic!("bubblewrap must work for the local check");
        };
        // Only a loopback interface exists, and it is down: nothing connects,
        // not even to a listener on the host's own 127.0.0.1 (which a
        // sandbox sharing the host's network would reach; no traffic leaves
        // the machine either way).
        let host = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = host.local_addr().unwrap().port();
        let (_, out) = sh(
            &sb,
            &root,
            &format!(
                "cat /proc/net/dev | tail -n +3 | cut -d: -f1 | tr -d ' '; (exec 3<>/dev/tcp/127.0.0.1/{port}) 2>&1 && echo connected"
            ),
        );
        assert!(
            !out.contains("connected\n") && !out.trim_end().ends_with("connected"),
            "{out}"
        );
        assert!(out.lines().next().is_some_and(|l| l == "lo"), "{out}");
    }

    #[test]
    fn off_and_unavailable() {
        let d = tempfile::tempdir().unwrap();
        assert!(matches!(
            choose(false, cfg(d.path(), d.path(), true), None),
            Isolation::Off
        ));
        let home = d.path().to_owned();
        let i = choose(true, cfg(&home, &home.join("t"), true), Some(&home));
        assert!(
            matches!(&i, Isolation::Unavailable(e) if e.contains("home directory")),
            "{i:?}"
        );
        assert_eq!(i.status(), Some("UNISOLATED"));
    }
}
