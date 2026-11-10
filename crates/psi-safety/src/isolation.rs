//! The isolation boundary (psi-plan.md §8.4, D-07, D-08, D-30, D-44):
//! minimal bubblewrap on Linux, a Seatbelt profile on macOS. It stops a
//! command from writing outside the workspace and from reading credentials
//! ([`crate::secrets`]), and little more.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

/// What bash runs inside.
#[derive(Debug, Clone)]
pub enum Isolation {
    /// Every bash call runs inside the sandbox.
    On(Arc<Sandbox>),
    /// The user turned isolation off: something outside Psi (an eval
    /// container) is the boundary. Bash runs directly.
    Off,
    /// No backend works; bash is refused.
    Unavailable(String),
}

impl Isolation {
    pub fn sandbox(&self) -> Option<&Arc<Sandbox>> {
        match self {
            Isolation::On(s) => Some(s),
            _ => None,
        }
    }
}

/// What enforces the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// bubblewrap (Linux).
    Bwrap,
    /// `sandbox-exec` with a generated profile (macOS).
    Seatbelt,
}

impl Backend {
    /// The one this platform has, if any.
    pub fn native() -> Option<Self> {
        if cfg!(target_os = "linux") {
            Some(Backend::Bwrap)
        } else if cfg!(target_os = "macos") {
            Some(Backend::Seatbelt)
        } else {
            None
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Backend::Bwrap => "bwrap",
            Backend::Seatbelt => "seatbelt",
        }
    }
}

/// Temp directories Seatbelt leaves writable: macOS tools write their own
/// temp and cache files there whatever TMPDIR says (§8.4).
const SEATBELT_SHARED: [&str; 2] = ["/private/tmp", "/private/var/folders"];

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
    /// Credential files and directories ([`crate::secrets`]): empty or
    /// unreadable inside.
    pub hidden: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct Sandbox {
    backend: Backend,
    cfg: SandboxConfig,
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
        // macOS keeps caches here (Go's build cache among them).
        "Library/Caches",
    ]
    .iter()
    .map(|d| home.join(d))
    .collect()
}

impl Sandbox {
    /// Checks the directories; `/` and the home directory are refused as
    /// the root or a cache directory.
    pub fn new(
        backend: Backend,
        mut cfg: SandboxConfig,
        home: Option<&Path>,
    ) -> Result<Self, String> {
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
        Ok(Self { backend, cfg })
    }

    pub fn config(&self) -> &SandboxConfig {
        &self.cfg
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// The path a failed command's output says the boundary blocked, if
    /// any (§8.5). Each backend's denial has its own error text.
    pub fn blocked(&self, output: &str) -> Option<PathBuf> {
        let marker = match self.backend {
            Backend::Bwrap => "read-only file system",
            Backend::Seatbelt => "operation not permitted",
        };
        crate::blocked::blocked_path_with(output, &self.writable(), marker)
    }

    /// The program and arguments to put before the command.
    pub fn command(&self, cwd: &Path) -> (&'static str, Vec<OsString>) {
        match self.backend {
            Backend::Bwrap => ("bwrap", self.args(cwd)),
            Backend::Seatbelt => (
                "/usr/bin/sandbox-exec",
                vec!["-p".into(), self.profile().into()],
            ),
        }
    }

    /// Every directory a command inside may write.
    pub fn writable(&self) -> Vec<PathBuf> {
        let mut v = vec![self.cfg.root.clone()];
        v.extend(self.cfg.git_dirs.iter().cloned());
        v.extend(self.cfg.cache_dirs.iter().cloned());
        v.push(self.cfg.tmpdir.clone());
        match self.backend {
            Backend::Bwrap => v.push("/tmp".into()),
            Backend::Seatbelt => v.extend(SEATBELT_SHARED.iter().map(PathBuf::from)),
        }
        v.push("/dev".into());
        v
    }

    /// The Seatbelt profile (§8.4): everything allowed, then each write
    /// outside the writable directories denied (`require-not` makes that
    /// independent of rule order), then the hidden paths denied for reads
    /// and writes. Paths are resolved, since Seatbelt matches real paths
    /// (/tmp is /private/tmp).
    pub fn profile(&self) -> String {
        let dirs: Vec<String> = self
            .writable()
            .iter()
            .map(|d| format!("(subpath {})", sbpl(&crate::blocked::resolve_path(d))))
            .collect();
        let mut p = format!(
            "(version 1)\n(allow default)\n(deny file-write* (require-not (require-any\n  {})))\n",
            dirs.join("\n  ")
        );
        if !self.cfg.hidden.is_empty() {
            let hidden: Vec<String> = self
                .cfg
                .hidden
                .iter()
                .map(|d| format!("(subpath {})", sbpl(&crate::blocked::resolve_path(d))))
                .collect();
            p.push_str(&format!(
                "(deny file-read* file-write*\n  {})\n",
                hidden.join("\n  ")
            ));
        }
        if !self.cfg.network {
            p.push_str("(deny network*)\n");
        }
        p
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
        // /tmp is the session's TMPDIR, so files there last between calls
        // (models write /tmp by habit) and the host's /tmp stays out.
        push("--bind", &[&self.cfg.tmpdir, Path::new("/tmp")]);
        push("--bind", &[&self.cfg.root, &self.cfg.root]);
        for d in &self.cfg.git_dirs {
            push("--bind-try", &[d, d]);
        }
        for d in &self.cfg.cache_dirs {
            push("--bind-try", &[d, d]);
        }
        // After the writable directories, so none of them uncovers one:
        // an empty directory over a directory, /dev/null over a file. A
        // path that doesn't exist needs no cover (the mount would fail).
        for h in &self.cfg.hidden {
            let h = crate::blocked::resolve_path(h);
            if h.is_dir() {
                push("--tmpfs", &[&h]);
            } else if h.exists() {
                push("--ro-bind", &[Path::new("/dev/null"), &h]);
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
        match self.backend {
            Backend::Bwrap => self.check_bwrap(),
            Backend::Seatbelt => self.check_seatbelt(),
        }
    }

    fn check_seatbelt(&self) -> Result<(), String> {
        if let Err(e) = std::fs::create_dir_all(&self.cfg.tmpdir) {
            return Err(format!(
                "cannot create TMPDIR {}: {e}",
                self.cfg.tmpdir.display()
            ));
        }
        let (program, args) = self.command(&self.cfg.root);
        let out = Command::new(program)
            .args(args)
            .arg("/usr/bin/true")
            .current_dir(&self.cfg.root)
            .stdin(Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(format!(
                "sandbox-exec failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => Err(format!("cannot run {program}: {e}")),
        }
    }
    fn check_bwrap(&self) -> Result<(), String> {
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
/// else the platform's sandbox if its check passes, else unavailable, with
/// the reason (never a silent fallback).
pub fn choose(enabled: bool, cfg: SandboxConfig, home: Option<&Path>) -> Isolation {
    if !enabled {
        return Isolation::Off;
    }
    let Some(backend) = Backend::native() else {
        return Isolation::Unavailable("isolation is built for Linux and macOS only".into());
    };
    let sb = match Sandbox::new(backend, cfg, home) {
        Ok(s) => s,
        Err(e) => return Isolation::Unavailable(e),
    };
    match sb.check() {
        Ok(()) => Isolation::On(Arc::new(sb)),
        Err(e) => Isolation::Unavailable(e),
    }
}

/// `p` as an SBPL string literal.
fn sbpl(p: &Path) -> String {
    let s = p.to_string_lossy();
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
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
            hidden: vec![],
        }
    }

    fn sh(sb: &Sandbox, cwd: &Path, script: &str) -> (bool, String) {
        let (program, args) = sb.command(cwd);
        let out = Command::new(program)
            .args(args)
            .args(["bash", "-c", script])
            .current_dir(cwd)
            // As the bash tool sets it; else $TMPDIR is the host's.
            .env("TMPDIR", &sb.config().tmpdir)
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
            hidden: vec![],
        };
        assert!(Sandbox::new(Backend::Bwrap, c, Some(home)).is_err());
        let c = SandboxConfig {
            root: home.join("p"),
            git_dirs: vec![],
            cache_dirs: vec![home.to_owned(), home.join(".cache")],
            tmpdir: "/x".into(),
            network: true,
            hidden: vec![],
        };
        let sb = Sandbox::new(Backend::Bwrap, c, Some(home)).unwrap();
        assert_eq!(sb.config().cache_dirs, [home.join(".cache")]);
    }

    #[test]
    fn tmpdir_is_bound_last() {
        let sb = Sandbox::new(
            Backend::Bwrap,
            cfg(Path::new("/w"), Path::new("/t"), false),
            None,
        )
        .unwrap();
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

    /// The Seatbelt profile allows only the writable directories, resolved,
    /// and denies the network when it is off.
    #[test]
    fn seatbelt_profile() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let mut c = cfg(&root, &d.path().join("t"), false);
        c.hidden = vec!["/opt/we \"x\"".into()];
        let sb = Sandbox::new(Backend::Seatbelt, c, None).unwrap();
        let p = sb.profile();
        assert!(p.starts_with("(version 1)\n(allow default)\n(deny file-write* (require-not"));
        let real = root.canonicalize().unwrap();
        assert!(
            p.contains(&format!("(subpath \"{}\")", real.display())),
            "{p}"
        );
        assert!(
            p.contains("(deny file-read* file-write*\n  (subpath \"/opt/we \\\"x\\\"\"))\n"),
            "{p}"
        );
        for dir in ["/private/tmp", "/private/var/folders", "/dev"] {
            assert!(p.contains(&format!("(subpath \"{dir}\")")), "{p}");
        }
        assert!(!p.contains("(subpath \"/tmp\")"), "{p}");
        assert!(p.ends_with("(deny network*)\n"), "{p}");
        let (program, args) = sb.command(&root);
        assert_eq!(program, "/usr/bin/sandbox-exec");
        assert_eq!(args.len(), 2);
        let on = Sandbox::new(
            Backend::Seatbelt,
            cfg(&root, &d.path().join("t"), true),
            None,
        )
        .unwrap();
        assert!(!on.profile().contains("network"));
    }

    // The boundary itself, under the real sandbox: bwrap on Linux (local
    // check D-33), Seatbelt on macOS (run by hand there).
    /// Not under /tmp, which is private inside the sandbox.
    #[cfg(target_os = "linux")]
    fn outside_tmp() -> tempfile::TempDir {
        let t = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        tempfile::tempdir_in(t).unwrap()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn writes_outside_fail_and_tmp_is_private() {
        let d = outside_tmp();
        let root = d.path().join("root");
        let tmp = d.path().join("tmpdir");
        let outside = d.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        // Credentials: a directory, a file, and one that doesn't exist.
        let keys = d.path().join("keys");
        let token = d.path().join("token");
        std::fs::create_dir_all(&keys).unwrap();
        std::fs::write(keys.join("id"), "secret-key").unwrap();
        std::fs::write(&token, "secret-token").unwrap();
        let mut c = cfg(&root, &tmp, true);
        c.hidden = vec![keys.clone(), token.clone(), d.path().join("absent")];
        let sb = match choose(true, c, None) {
            Isolation::On(s) => s,
            other => panic!("bubblewrap must work for the local check: {other:?}"),
        };
        let (_, out) = sh(
            &sb,
            &root,
            &format!(
                "(ls -A {k}; cat {k}/id; cat {t}) 2>&1; echo end",
                k = keys.display(),
                t = token.display()
            ),
        );
        assert!(!out.contains("secret"), "{out}");
        // The directory is empty; the file is /dev/null, unreadable.
        assert!(
            out.contains("No such file") && out.contains("Permission denied"),
            "{out}"
        );
        assert!(out.ends_with("end\n"), "{out}");
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
        // /tmp is the session's TMPDIR: the next call sees the file, the
        // host's /tmp doesn't.
        let (ok, out) = sh(&sb, &root, "cat /tmp/private");
        assert!(ok, "{out}");
        assert_eq!(out.trim(), "z");
        assert_eq!(std::fs::read_to_string(tmp.join("private")).unwrap(), "z\n");
        assert!(!Path::new("/tmp/private").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn network_off_blocks_traffic() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let Isolation::On(sb) = choose(true, cfg(&root, &d.path().join("t"), false), None) else {
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

    /// Writes outside fail with the text `blocked` looks for; the workspace,
    /// TMPDIR, the shared /tmp and an added directory are writable.
    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_blocks_writes_outside() {
        // /var/tmp: outside the temp directories Seatbelt leaves writable.
        let d = tempfile::tempdir_in("/var/tmp").unwrap();
        let root = d.path().join("root");
        let tmp = d.path().join("tmpdir");
        let outside = d.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let keys = d.path().join("keys");
        std::fs::create_dir_all(&keys).unwrap();
        std::fs::write(keys.join("id"), "secret-key").unwrap();
        let mut c = cfg(&root, &tmp, true);
        c.hidden = vec![keys.clone(), d.path().join("absent")];
        let sb = match choose(true, c, None) {
            Isolation::On(s) => s,
            other => panic!("Seatbelt must work on macOS: {other:?}"),
        };
        let (ok, out) = sh(&sb, &root, &format!("cat {}/id", keys.display()));
        assert!(!ok && !out.contains("secret"), "{out}");
        assert_eq!(sb.backend(), Backend::Seatbelt);
        let target = outside.join("f");
        let (ok, out) = sh(&sb, &root, &format!("echo x > {}", target.display()));
        assert!(!ok, "{out}");
        assert!(out.contains("Operation not permitted"), "{out}");
        assert!(!target.exists());
        assert_eq!(
            sb.blocked(&out),
            Some(crate::blocked::resolve_path(&target)),
            "{out}"
        );
        let marker = format!("/tmp/psi-seatbelt-{}", std::process::id());
        let (ok, out) = sh(
            &sb,
            &root,
            &format!("echo x > inside && echo y > \"$TMPDIR\"/kept && echo z > {marker}"),
        );
        assert!(ok, "{out}");
        assert!(root.join("inside").exists());
        assert!(tmp.join("kept").exists());
        // Shared, unlike bwrap's: the host sees it.
        assert!(Path::new(&marker).exists());
        let _ = std::fs::remove_file(&marker);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_network_off_blocks_traffic() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let Isolation::On(sb) = choose(true, cfg(&root, &d.path().join("t"), false), None) else {
            panic!("Seatbelt must work on macOS");
        };
        // Not even the host's own 127.0.0.1 (no traffic leaves the machine
        // either way).
        let host = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = host.local_addr().unwrap().port();
        let (_, out) = sh(
            &sb,
            &root,
            &format!("(exec 3<>/dev/tcp/127.0.0.1/{port}) 2>&1 && echo connected"),
        );
        assert!(!out.trim_end().ends_with("connected"), "{out}");
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
    }
}
