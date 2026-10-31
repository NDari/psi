//! Running trial and grading containers with podman.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{IoContext, other};
use crate::{Result, fsutil};

/// The trial image, built from `evals/Containerfile`.
pub const IMAGE: &str = "localhost/psi-eval:latest";

/// Builds (or refreshes from cache) the trial image. The build output goes
/// to `log`.
pub fn build_image(evals_dir: &Path, log: &Path) -> Result<()> {
    let out = File::create(log).ctx(|| format!("creating {}", log.display()))?;
    let err = out
        .try_clone()
        .ctx(|| format!("opening {}", log.display()))?;
    let status = Command::new("podman")
        .args(["build", "-t", IMAGE, "-f"])
        .arg(evals_dir.join("Containerfile"))
        .arg(evals_dir)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .status()
        .ctx(|| "running podman build".into())?;
    if !status.success() {
        let tail = fsutil::read_to_string(log).unwrap_or_default();
        let tail: Vec<&str> = tail.lines().rev().take(10).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(other(format!("podman build failed:\n{}", tail.join("\n"))));
    }
    Ok(())
}

/// One container run: `podman run --rm` as the invoking user, with a home
/// at `/home/eval`.
#[derive(Debug, Clone)]
pub(crate) struct Container {
    pub network: String,
    pub home: PathBuf,
    /// (host path, container path, read-only)
    pub mounts: Vec<(PathBuf, String, bool)>,
    pub env: Vec<(String, String)>,
    pub workdir: Option<String>,
    pub cmd: Vec<String>,
}

impl Container {
    pub fn new(network: &str, home: &Path) -> Self {
        Self {
            network: network.to_owned(),
            home: absolute(home),
            mounts: Vec::new(),
            env: Vec::new(),
            workdir: None,
            cmd: Vec::new(),
        }
    }

    pub fn mount(mut self, host: &Path, container: &str, read_only: bool) -> Self {
        self.mounts
            .push((absolute(host), container.to_owned(), read_only));
        self
    }

    pub fn env(mut self, k: &str, v: &str) -> Self {
        self.env.push((k.to_owned(), v.to_owned()));
        self
    }

    pub fn workdir(mut self, dir: &str) -> Self {
        self.workdir = Some(dir.to_owned());
        self
    }

    pub fn cmd<S: AsRef<str>>(mut self, args: impl IntoIterator<Item = S>) -> Self {
        self.cmd = args.into_iter().map(|s| s.as_ref().to_owned()).collect();
        self
    }

    pub fn args(&self) -> Vec<String> {
        let mut a: Vec<String> = vec![
            "run".into(),
            "--rm".into(),
            "--userns=keep-id".into(),
            "--network".into(),
            self.network.clone(),
            "-v".into(),
            format!("{}:/home/eval", self.home.display()),
            "-e".into(),
            "HOME=/home/eval".into(),
            "-e".into(),
            "TERM=dumb".into(),
        ];
        for (host, ctr, ro) in &self.mounts {
            a.push("-v".into());
            let suffix = if *ro { ":ro" } else { "" };
            a.push(format!("{}:{ctr}{suffix}", host.display()));
        }
        for (k, v) in &self.env {
            a.push("-e".into());
            a.push(format!("{k}={v}"));
        }
        if let Some(w) = &self.workdir {
            a.push("-w".into());
            a.push(w.clone());
        }
        a.push(IMAGE.into());
        a.extend(self.cmd.iter().cloned());
        a
    }

    /// Runs to completion with stdin closed and output in the given files.
    /// Returns the exit code, or -1 if podman was killed by a signal.
    pub fn run(&self, stdout: &Path, stderr: &Path) -> Result<i32> {
        let out = File::create(stdout).ctx(|| format!("creating {}", stdout.display()))?;
        let err = File::create(stderr).ctx(|| format!("creating {}", stderr.display()))?;
        tracing::debug!(args = ?self.args(), "podman");
        let status = Command::new("podman")
            .args(self.args())
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .status()
            .ctx(|| "running podman".into())?;
        Ok(status.code().unwrap_or(-1))
    }
}

/// podman reads a relative `-v` source as a named volume, so every host
/// path is made absolute.
fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_put_mounts_env_and_workdir_before_the_image() {
        let c = Container::new("none", Path::new("/h"))
            .mount(Path::new("/p"), "/grade", false)
            .mount(Path::new("/x"), "/hidden", true)
            .env("A", "1")
            .workdir("/grade")
            .cmd(["bash", "/hidden/check.sh"]);
        let a = c.args().join(" ");
        assert_eq!(
            a,
            "run --rm --userns=keep-id --network none -v /h:/home/eval -e HOME=/home/eval \
             -e TERM=dumb -v /p:/grade -v /x:/hidden:ro -e A=1 -w /grade \
             localhost/psi-eval:latest bash /hidden/check.sh"
        );
    }

    #[test]
    fn relative_host_paths_become_absolute() {
        let c = Container::new("none", Path::new("h")).mount(Path::new("evals/x"), "/x", true);
        let cwd = std::env::current_dir().unwrap();
        let a = c.args();
        assert!(a.contains(&format!("{}:/home/eval", cwd.join("h").display())));
        assert!(a.contains(&format!("{}:/x:ro", cwd.join("evals/x").display())));
    }
}
