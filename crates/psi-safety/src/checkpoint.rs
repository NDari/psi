//! Shadow-git checkpoints (psi-plan.md §7.2): snapshots of the project's work
//! tree in a bare repository outside the workspace, so any change the agent
//! makes, by file tools or bash, can be undone. The project's own `.git` is
//! never touched or read.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::git::{git_available, host_git};

/// Files larger than this are not captured.
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// Stored bytes are the bytes on disk: no line-ending, filter or encoding conversion.
const ATTRIBUTES: &str = "* -text -filter -ident -working-tree-encoding\n";
/// Config for the shadow repository, on top of `host_git`'s. The global and
/// user files (excludes, attributes) are off, so only in-tree `.gitignore` counts.
const SHADOW_CONFIG: [&str; 7] = [
    "core.attributesFile=/dev/null",
    "core.excludesFile=",
    "core.autocrlf=false",
    "core.safecrlf=false",
    "core.quotePath=false",
    "user.name=psi",
    "user.email=psi@localhost",
];
const NESTED: &str = "nested repository";
const ABSENT: &str = "000000";

#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("git is not installed, so checkpoints are off")]
    GitMissing,
    #[error("invalid session id {0:?}: use only letters, digits, '-' and '_'")]
    BadSession(String),
    #[error("git {step} failed: {stderr}")]
    Git { step: &'static str, stderr: String },
    #[error("unexpected output from git {0}")]
    Parse(&'static str),
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: io::Error,
    },
}

type Result<T> = std::result::Result<T, CheckpointError>;

fn io_err(context: &'static str) -> impl FnOnce(io::Error) -> CheckpointError {
    move |source| CheckpointError::Io { context, source }
}

/// One session's view of a project's shadow repository.
#[derive(Debug, Clone)]
pub struct Checkpoints {
    git_dir: PathBuf,
    root: PathBuf,
    index: PathBuf,
    refname: String,
}

/// The result of a capture.
#[derive(Debug, Clone)]
pub struct Capture {
    pub commit: String,
    /// False when the tree equals the head's, so no commit was made.
    pub changed: bool,
    /// Paths not captured, with the reason.
    pub skipped: Vec<(PathBuf, &'static str)>,
    pub elapsed: Duration,
}

/// The result of a restore. Paths are relative to the root.
#[derive(Debug, Clone, Default)]
pub struct Restore {
    /// The commit captured just before restoring; restoring it undoes the restore.
    pub before: String,
    pub written: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
    pub left_alone: Vec<(PathBuf, &'static str)>,
}

/// A file that differs between two commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// 'A', 'M', 'D' or 'T' (type change).
    pub status: char,
    pub path: PathBuf,
}

/// One `diff-tree --raw` entry.
struct RawChange {
    new_mode: String,
    new_oid: String,
    status: char,
    path: PathBuf,
}

impl Checkpoints {
    /// Opens, or creates, the shadow repository for `root` under `store`.
    pub fn open(store: &Path, root: &Path, session: &str) -> Result<Self> {
        let valid = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_';
        if session.is_empty() || !session.bytes().all(valid) {
            return Err(CheckpointError::BadSession(session.to_string()));
        }
        if !git_available() {
            return Err(CheckpointError::GitMissing);
        }
        let root = root
            .canonicalize()
            .map_err(io_err("resolving the project root"))?;
        let git_dir = store.join(format!("{:016x}", fnv1a(root.as_os_str().as_bytes())));
        if !git_dir.join("HEAD").is_file() {
            fs::create_dir_all(&git_dir).map_err(io_err("creating the checkpoint store"))?;
            let mut init = host_git();
            init.args([
                "-c",
                "init.defaultBranch=psi",
                "init",
                "--quiet",
                "--bare",
                "--template=",
            ])
            .arg(&git_dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
            ok(run_cmd(init, "init", None)?, "init")?;
        }
        let info = git_dir.join("info");
        fs::create_dir_all(&info).map_err(io_err("creating the checkpoint store"))?;
        fs::write(info.join("attributes"), ATTRIBUTES)
            .map_err(io_err("writing info/attributes"))?;
        Ok(Self {
            index: git_dir.join(format!("psi-index-{session}")),
            refname: format!("refs/psi/{session}"),
            git_dir,
            root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// The session's latest checkpoint commit, if any.
    pub fn head(&self) -> Option<String> {
        self.head_and_tree()
            .ok()
            .flatten()
            .map(|(commit, _)| commit)
    }

    /// Snapshots the work tree. When the tree equals the head's, nothing is
    /// stored and `commit` is the head. A session's first capture always commits.
    pub fn capture(&self, message: &str) -> Result<Capture> {
        let start = Instant::now();

        // 1. Find files to skip among the new and changed ones.
        let listed = self.run(
            "ls-files",
            [
                "ls-files",
                "--others",
                "--modified",
                "--exclude-standard",
                "-z",
            ],
            None,
        )?;
        let mut skipped: Vec<(PathBuf, &'static str)> = Vec::new();
        let mut nested = HashSet::new();
        let mut remove = Vec::new();
        for raw in listed.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            // An untracked nested repository is listed as `dir/`.
            if let Some(dir) = raw.strip_suffix(b"/") {
                let dir = PathBuf::from(OsStr::from_bytes(dir));
                if nested.insert(dir.clone()) {
                    skipped.push((dir, NESTED));
                }
                continue;
            }
            let rel = PathBuf::from(OsStr::from_bytes(raw));
            // A tracked directory that later became a repository is listed file by file.
            if let Some(dir) = self.nested_ancestor(&rel) {
                if nested.insert(dir.clone()) {
                    skipped.push((dir, NESTED));
                }
                continue;
            }
            let path = self.root.join(&rel);
            let reason = match fs::symlink_metadata(&path) {
                Ok(m) if m.is_file() && m.len() > MAX_FILE_BYTES => "over 50 MiB",
                Ok(m) if m.is_file() && fs::File::open(&path).is_err() => "unreadable",
                _ => continue,
            };
            remove.extend_from_slice(raw);
            remove.push(0);
            skipped.push((rel, reason));
        }

        // 2. Drop skipped files from the index, in case an earlier capture had them.
        if !remove.is_empty() {
            self.run(
                "update-index",
                ["update-index", "--force-remove", "-z", "--stdin"],
                Some(&remove),
            )?;
        }

        // 3. Stage everything else.
        let mut pathspec = b".\0".to_vec();
        for (path, _) in &skipped {
            pathspec.extend_from_slice(b":(exclude,literal)");
            pathspec.extend_from_slice(path.as_os_str().as_bytes());
            pathspec.push(0);
        }
        self.run(
            "add",
            ["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
            Some(&pathspec),
        )?;

        // 4. Write the tree; store nothing if it is unchanged.
        let tree = text(self.run("write-tree", ["write-tree"], None)?);
        let head = self.head_and_tree()?;
        if let Some((commit, head_tree)) = &head
            && *head_tree == tree
        {
            let elapsed = start.elapsed();
            tracing::debug!(
                ?elapsed,
                changed = false,
                skipped = skipped.len(),
                "checkpoint"
            );
            return Ok(Capture {
                commit: commit.clone(),
                changed: false,
                skipped,
                elapsed,
            });
        }

        // 5. Commit it.
        let mut args = vec!["commit-tree", tree.as_str()];
        if let Some((parent, _)) = &head {
            args.extend(["-p", parent.as_str()]);
        }
        args.extend(["-m", message]);
        let commit = text(self.run("commit-tree", args, None)?);

        // 6. Move the session ref, only if no other capture moved it meanwhile.
        let old = head.as_ref().map_or("", |(c, _)| c.as_str());
        self.run(
            "update-ref",
            ["update-ref", self.refname.as_str(), commit.as_str(), old],
            None,
        )?;

        let elapsed = start.elapsed();
        tracing::debug!(
            ?elapsed,
            changed = true,
            skipped = skipped.len(),
            "checkpoint"
        );
        Ok(Capture {
            commit,
            changed: true,
            skipped,
            elapsed,
        })
    }

    /// Makes the work tree match `commit`. Captures first, so the restore can
    /// be undone (`before`), and again after. Leaves alone `.git`, nested
    /// repositories, ignored paths, paths below a symlink and uncaptured files
    /// in the way.
    pub fn restore(&self, commit: &str) -> Result<Restore> {
        let target = self.resolve(commit)?;
        let before = self.capture(&format!("before restoring {}", short(&target)))?;
        let changes = self.raw_changes(&before.commit, &target)?;
        let (mut written, mut deleted, mut left_alone) = (Vec::new(), Vec::new(), Vec::new());
        let mut leave = |path: &Path, why: &'static str| left_alone.push((path.to_path_buf(), why));

        // Deletions first, so a file can replace a directory and the reverse.
        for change in changes.iter().filter(|c| c.new_mode == ABSENT) {
            if let Some(why) = self.blocker(&change.path) {
                leave(&change.path, why);
                continue;
            }
            let path = self.root.join(&change.path);
            match fs::symlink_metadata(&path) {
                Ok(m) if m.is_dir() => leave(&change.path, "a directory in the way"),
                Ok(_) => match fs::remove_file(&path) {
                    Ok(()) => {
                        self.remove_empty_parents(&path);
                        deleted.push(change.path.clone());
                    }
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "restore: delete failed");
                        leave(&change.path, "could not delete");
                    }
                },
                Err(_) => {}
            }
        }

        let mut blobs: Option<Blobs> = None;
        for change in changes.iter().filter(|c| c.new_mode != ABSENT) {
            if change.new_mode == "160000" {
                leave(&change.path, "submodule");
                continue;
            }
            if let Some(why) = self.blocker(&change.path) {
                leave(&change.path, why);
                continue;
            }
            let path = self.root.join(&change.path);
            let existing = fs::symlink_metadata(&path).ok();
            if existing.is_some() && change.status == 'A' {
                // Not in the capture just taken, so not ours to overwrite.
                let why = before
                    .skipped
                    .iter()
                    .find(|(p, _)| change.path.starts_with(p))
                    .map_or("untracked file in the way", |(_, why)| *why);
                leave(&change.path, why);
                continue;
            }
            if existing.as_ref().is_some_and(|m| m.is_dir()) {
                leave(&change.path, "a directory in the way");
                continue;
            }
            let blobs = match &mut blobs {
                Some(b) => b,
                None => blobs.insert(Blobs::start(self.git())?),
            };
            let data = blobs.read(&change.new_oid)?;
            let replaced = (|| {
                if existing.is_some() {
                    fs::remove_file(&path)?;
                }
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                write_entry(&path, &change.new_mode, &data)
            })();
            match replaced {
                Ok(()) => written.push(change.path.clone()),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "restore: write failed");
                    leave(&change.path, "could not write");
                }
            }
        }
        drop(blobs);
        let out = Restore {
            before: before.commit.clone(),
            written,
            deleted,
            left_alone,
        };

        self.capture(&format!("restored {}", short(&target)))?;
        tracing::debug!(
            written = out.written.len(),
            deleted = out.deleted.len(),
            left_alone = out.left_alone.len(),
            "restore"
        );
        Ok(out)
    }

    /// Files that differ between two commits.
    pub fn changes(&self, from: &str, to: &str) -> Result<Vec<FileChange>> {
        Ok(self
            .raw_changes(from, to)?
            .into_iter()
            .map(|c| FileChange {
                status: c.status,
                path: c.path,
            })
            .collect())
    }

    /// A plain unified diff between two commits. Callers cut it to size.
    pub fn diff(&self, from: &str, to: &str) -> Result<String> {
        let out = self.run(
            "diff",
            [
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--end-of-options",
                from,
                to,
            ],
            None,
        )?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// Whether a path is gitignored and not captured, so checkpoints can't
    /// recover a write to it. Relative paths are taken from the root; paths
    /// outside the root return false.
    pub fn is_ignored(&self, path: &Path) -> bool {
        let Some(rel) = self.relative(path) else {
            return false;
        };
        let mut cmd = self.git();
        cmd.args(["check-ignore", "-q", "--"]).arg(rel);
        matches!(run_cmd(cmd, "check-ignore", None), Ok(out) if out.status.success())
    }

    /// A `git` command on the shadow repository with this session's index.
    fn git(&self) -> Command {
        let mut cmd = host_git();
        for setting in SHADOW_CONFIG {
            cmd.args(["-c", setting]);
        }
        cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_DIR", &self.git_dir)
            .env("GIT_WORK_TREE", &self.root)
            .env("GIT_INDEX_FILE", &self.index)
            .current_dir(&self.root);
        cmd
    }

    fn run<I, S>(&self, step: &'static str, args: I, input: Option<&[u8]>) -> Result<Vec<u8>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.git();
        cmd.args(args);
        ok(run_cmd(cmd, step, input)?, step)
    }

    /// The head commit and its tree, or None before the session's first capture.
    fn head_and_tree(&self) -> Result<Option<(String, String)>> {
        let out = self.run(
            "for-each-ref",
            [
                "for-each-ref",
                "--format=%(objectname) %(tree)",
                self.refname.as_str(),
            ],
            None,
        )?;
        let out = text(out);
        let line = out.lines().next();
        match line.map(|l| l.split_once(' ')) {
            None => Ok(None),
            Some(Some((commit, tree))) => Ok(Some((commit.to_string(), tree.to_string()))),
            Some(None) => Err(CheckpointError::Parse("for-each-ref")),
        }
    }

    fn resolve(&self, commit: &str) -> Result<String> {
        let spec = format!("{commit}^{{commit}}");
        let args = [
            "rev-parse",
            "--verify",
            "-q",
            "--end-of-options",
            spec.as_str(),
        ];
        self.run("rev-parse", args, None)
            .map(text)
            .map_err(|_| CheckpointError::Git {
                step: "rev-parse",
                stderr: format!("no checkpoint commit {commit:?}"),
            })
    }

    fn raw_changes(&self, from: &str, to: &str) -> Result<Vec<RawChange>> {
        let out = self.run(
            "diff-tree",
            [
                "diff-tree",
                "-r",
                "--no-renames",
                "-z",
                "--end-of-options",
                from,
                to,
            ],
            None,
        )?;
        // Each entry is `:old_mode new_mode old_oid new_oid status` NUL `path` NUL.
        let mut fields = out.split(|b| *b == 0).filter(|f| !f.is_empty());
        let mut changes = Vec::new();
        while let Some(meta) = fields.next() {
            let bad = || CheckpointError::Parse("diff-tree");
            let path = fields.next().ok_or_else(bad)?;
            let meta = std::str::from_utf8(meta).map_err(|_| bad())?;
            let parts: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
            let [_, new_mode, _, new_oid, status] = parts[..] else {
                return Err(bad());
            };
            changes.push(RawChange {
                new_mode: new_mode.to_string(),
                new_oid: new_oid.to_string(),
                status: status.chars().next().ok_or_else(bad)?,
                path: PathBuf::from(OsStr::from_bytes(path)),
            });
        }
        Ok(changes)
    }

    /// The outermost directory above `rel` that holds a `.git`, if any.
    fn nested_ancestor(&self, rel: &Path) -> Option<PathBuf> {
        rel.ancestors()
            .skip(1)
            .filter(|a| !a.as_os_str().is_empty())
            .filter(|a| fs::symlink_metadata(self.root.join(a).join(".git")).is_ok())
            .last()
            .map(Path::to_path_buf)
    }

    /// Why a restore must not touch `rel`, judged by the directories above it.
    fn blocker(&self, rel: &Path) -> Option<&'static str> {
        if rel.components().any(|c| c.as_os_str() == ".git") {
            return Some("inside .git");
        }
        let mut dir = self.root.clone();
        let parents: Vec<_> = rel.components().collect();
        for part in parents.iter().take(parents.len().saturating_sub(1)) {
            dir.push(part);
            match fs::symlink_metadata(&dir) {
                Ok(m) if m.file_type().is_symlink() => return Some("below a symlink"),
                Ok(m) if !m.is_dir() => return Some("untracked file in the way"),
                Ok(_) if fs::symlink_metadata(dir.join(".git")).is_ok() => return Some(NESTED),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        None
    }

    fn remove_empty_parents(&self, path: &Path) {
        let mut dir = path.parent();
        while let Some(d) = dir {
            if d == self.root || !d.starts_with(&self.root) || fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }

    /// `path` relative to the root, or None if it lies outside.
    fn relative(&self, path: &Path) -> Option<PathBuf> {
        let abs = self.root.join(path);
        let lexical = !abs.components().any(|c| c == Component::ParentDir);
        if lexical && let Ok(rel) = abs.strip_prefix(&self.root) {
            return (!rel.as_os_str().is_empty()).then(|| rel.to_path_buf());
        }
        // Resolve `..` and symlinks through the parent, which must exist.
        let parent = abs.parent()?.canonicalize().ok()?;
        let rel = parent
            .join(abs.file_name()?)
            .strip_prefix(&self.root)
            .ok()?
            .to_path_buf();
        (!rel.as_os_str().is_empty()).then_some(rel)
    }
}

/// Creates a file or symlink with the stored mode. The path must not exist.
fn write_entry(path: &Path, mode: &str, data: &[u8]) -> io::Result<()> {
    if mode == "120000" {
        return std::os::unix::fs::symlink(OsStr::from_bytes(data), path);
    }
    let perms = if mode == "100755" { 0o777 } else { 0o666 };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(perms)
        .open(path)?;
    file.write_all(data)
}

/// One `git cat-file --batch` process serving every blob of a restore.
struct Blobs {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Blobs {
    fn start(mut cmd: Command) -> Result<Self> {
        let mut child = cmd
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(spawn_err)?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(CheckpointError::Parse("cat-file"));
        };
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// One request at a time, so neither pipe can fill up and deadlock.
    fn read(&mut self, oid: &str) -> Result<Vec<u8>> {
        writeln!(self.stdin, "{oid}")
            .and_then(|()| self.stdin.flush())
            .map_err(io_err("writing to git cat-file"))?;
        let mut header = String::new();
        self.stdout
            .read_line(&mut header)
            .map_err(io_err("reading from git cat-file"))?;
        // `<oid> <type> <size>`, or `<oid> missing`.
        let size: usize = header
            .split_whitespace()
            .nth(2)
            .and_then(|s| s.parse().ok())
            .ok_or(CheckpointError::Parse("cat-file"))?;
        let mut data = vec![0; size + 1];
        self.stdout
            .read_exact(&mut data)
            .map_err(io_err("reading from git cat-file"))?;
        data.truncate(size); // the trailing newline
        Ok(data)
    }
}

impl Drop for Blobs {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_err(e: io::Error) -> CheckpointError {
    if e.kind() == io::ErrorKind::NotFound {
        CheckpointError::GitMissing
    } else {
        CheckpointError::Io {
            context: "running git",
            source: e,
        }
    }
}

/// Runs `cmd`, feeding `input` on stdin from a thread so large inputs can't deadlock.
fn run_cmd(mut cmd: Command, step: &'static str, input: Option<&[u8]>) -> Result<Output> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().map_err(spawn_err)?;
    let stdin = child.stdin.take();
    let out = std::thread::scope(|s| {
        if let (Some(data), Some(mut stdin)) = (input, stdin) {
            // A write error means git exited early; its status says why.
            s.spawn(move || stdin.write_all(data));
        }
        child.wait_with_output()
    });
    out.map_err(|e| {
        tracing::warn!(step, error = %e, "git failed to run");
        CheckpointError::Io {
            context: "running git",
            source: e,
        }
    })
}

fn ok(out: Output, step: &'static str) -> Result<Vec<u8>> {
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(CheckpointError::Git { step, stderr })
    }
}

fn text(out: Vec<u8>) -> String {
    String::from_utf8_lossy(&out).trim().to_string()
}

fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

/// FNV-1a 64: a hash that stays stable across Rust versions, for store names.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    struct Fixture {
        _tmp: tempfile::TempDir,
        base: PathBuf,
        root: PathBuf,
        store: PathBuf,
    }

    /// A project that is a git repository with one commit, and an empty store.
    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = base.join("project");
        fs::create_dir(&root).unwrap();
        git_in(&root, &["init", "-q"]);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        git_in(&root, &["add", "a.txt"]);
        git_in(&root, &["commit", "-q", "-m", "init"]);
        let store = base.join("store");
        Fixture {
            _tmp: tmp,
            base,
            root,
            store,
        }
    }

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

    fn files(cp: &Checkpoints, commit: &str) -> Vec<String> {
        let out = cp
            .run("ls-tree", ["ls-tree", "-r", "--name-only", commit], None)
            .unwrap();
        text(out).lines().map(String::from).collect()
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn is_exec(path: &Path) -> bool {
        fs::metadata(path).unwrap().permissions().mode() & 0o111 != 0
    }

    #[test]
    fn unchanged_tree_makes_no_commit() {
        let f = fixture();
        let cp = Checkpoints::open(&f.store, &f.root, "s1").unwrap();
        assert_eq!(cp.head(), None);
        let first = cp.capture("first").unwrap();
        assert!(first.changed);
        assert_eq!(cp.head().as_deref(), Some(first.commit.as_str()));
        let second = cp.capture("second").unwrap();
        assert!(!second.changed);
        assert_eq!(second.commit, first.commit);
        assert_eq!(files(&cp, &first.commit), ["a.txt"]);
    }

    #[test]
    fn bad_session_ids_are_refused() {
        let f = fixture();
        for bad in ["", "a/b", "..", "x y"] {
            assert!(matches!(
                Checkpoints::open(&f.store, &f.root, bad),
                Err(CheckpointError::BadSession(_))
            ));
        }
    }

    #[test]
    fn capture_and_restore_round_trip() {
        let f = fixture();
        let r = &f.root;
        fs::write(r.join("del.txt"), "gone soon\n").unwrap();
        fs::write(r.join("run.sh"), "#!/bin/sh\n").unwrap();
        fs::create_dir_all(r.join("sub/deep")).unwrap();
        fs::write(r.join("sub/deep/x.txt"), "deep\n").unwrap();
        let cp = Checkpoints::open(&f.store, r, "s1").unwrap();
        let c1 = cp.capture("c1").unwrap().commit;

        fs::write(r.join("a.txt"), "two\n").unwrap();
        fs::write(r.join("new.txt"), "new\n").unwrap();
        fs::create_dir(r.join("fresh")).unwrap();
        fs::write(r.join("fresh/n.txt"), "n\n").unwrap();
        fs::remove_file(r.join("del.txt")).unwrap();
        fs::remove_dir_all(r.join("sub")).unwrap();
        fs::set_permissions(r.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        symlink("a.txt", r.join("link")).unwrap();
        let c2 = cp.capture("c2").unwrap();
        assert!(c2.changed);
        let mut changes = cp.changes(&c1, &c2.commit).unwrap();
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        let got: Vec<(char, &str)> = changes
            .iter()
            .map(|c| (c.status, c.path.to_str().unwrap()))
            .collect();
        assert_eq!(
            got,
            [
                ('M', "a.txt"),
                ('D', "del.txt"),
                ('A', "fresh/n.txt"),
                ('A', "link"),
                ('A', "new.txt"),
                ('M', "run.sh"),
                ('D', "sub/deep/x.txt"),
            ]
        );
        let diff = cp.diff(&c1, &c2.commit).unwrap();
        assert!(diff.contains("-one\n+two\n"), "{diff}");
        assert!(diff.contains("new mode 100755"), "{diff}");

        let undo = cp.restore(&c1).unwrap();
        assert_eq!(undo.before, c2.commit);
        assert!(undo.left_alone.is_empty(), "{:?}", undo.left_alone);
        assert_eq!(read(&r.join("a.txt")), "one\n");
        assert_eq!(read(&r.join("del.txt")), "gone soon\n");
        assert_eq!(read(&r.join("sub/deep/x.txt")), "deep\n");
        assert!(!is_exec(&r.join("run.sh")));
        assert!(!r.join("new.txt").exists());
        assert!(!r.join("fresh").exists(), "emptied directory is removed");
        assert!(fs::symlink_metadata(r.join("link")).is_err());
        assert_eq!(undo.deleted.len(), 3);
        assert_eq!(undo.written.len(), 4);
        // The tree now equals c1, so a capture stores nothing new.
        assert!(!cp.capture("after").unwrap().changed);
        assert_eq!(cp.changes(&c1, &cp.head().unwrap()).unwrap(), []);

        // Undo the undo.
        cp.restore(&undo.before).unwrap();
        assert_eq!(read(&r.join("a.txt")), "two\n");
        assert_eq!(read(&r.join("new.txt")), "new\n");
        assert_eq!(read(&r.join("fresh/n.txt")), "n\n");
        assert!(!r.join("del.txt").exists());
        assert!(!r.join("sub").exists());
        assert!(is_exec(&r.join("run.sh")));
        assert_eq!(fs::read_link(r.join("link")).unwrap(), Path::new("a.txt"));
    }

    #[test]
    fn large_and_unreadable_files_are_skipped() {
        let f = fixture();
        let big = fs::File::create(f.root.join("big.bin")).unwrap();
        big.set_len(MAX_FILE_BYTES + 1).unwrap(); // sparse, so fast
        fs::write(f.root.join("secret"), "s").unwrap();
        fs::set_permissions(f.root.join("secret"), fs::Permissions::from_mode(0o000)).unwrap();
        // Running as root can read anything; then only the size check applies.
        let unreadable = fs::File::open(f.root.join("secret")).is_err();

        let cp = Checkpoints::open(&f.store, &f.root, "s1").unwrap();
        let cap = cp.capture("c").unwrap();
        assert!(
            cap.skipped
                .contains(&(PathBuf::from("big.bin"), "over 50 MiB"))
        );
        assert_eq!(
            cap.skipped
                .contains(&(PathBuf::from("secret"), "unreadable")),
            unreadable
        );
        assert!(!files(&cp, &cap.commit).contains(&"big.bin".to_string()));

        // A captured file that grows too large drops out of the next capture.
        fs::set_permissions(f.root.join("secret"), fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_file(f.root.join("big.bin")).unwrap();
        let c2 = cp.capture("c2").unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(f.root.join("a.txt"))
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let c3 = cp.capture("c3").unwrap();
        assert!(
            c3.skipped
                .contains(&(PathBuf::from("a.txt"), "over 50 MiB"))
        );
        let changes = cp.changes(&c2.commit, &c3.commit).unwrap();
        assert_eq!(
            changes,
            [FileChange {
                status: 'D',
                path: "a.txt".into()
            }]
        );
        // And restore leaves the large file alone rather than overwrite it.
        let res = cp.restore(&c2.commit).unwrap();
        assert_eq!(res.left_alone, [(PathBuf::from("a.txt"), "over 50 MiB")]);
        assert_eq!(
            fs::metadata(f.root.join("a.txt")).unwrap().len(),
            MAX_FILE_BYTES + 1
        );
    }

    #[test]
    fn nested_repositories_are_skipped_and_left_alone() {
        let f = fixture();
        let lib = f.root.join("vendor/lib");
        fs::create_dir_all(&lib).unwrap();
        git_in(&lib, &["init", "-q"]);
        fs::write(lib.join("f"), "v1\n").unwrap();
        // A tracked directory that becomes a repository later.
        fs::create_dir(f.root.join("n")).unwrap();
        fs::write(f.root.join("n/y"), "y1\n").unwrap();

        let cp = Checkpoints::open(&f.store, &f.root, "s1").unwrap();
        let c1 = cp.capture("c1").unwrap();
        assert!(c1.skipped.contains(&(PathBuf::from("vendor/lib"), NESTED)));
        assert_eq!(files(&cp, &c1.commit), ["a.txt", "n/y"]);

        git_in(&f.root.join("n"), &["init", "-q"]);
        fs::write(f.root.join("n/y"), "y2\n").unwrap();
        fs::write(f.root.join("n/z"), "z\n").unwrap();
        fs::write(lib.join("f"), "v2\n").unwrap();
        let c2 = cp.capture("c2").unwrap();
        assert!(c2.skipped.contains(&(PathBuf::from("n"), NESTED)));
        assert!(!files(&cp, &c2.commit).contains(&"n/z".to_string()));

        fs::write(f.root.join("a.txt"), "changed\n").unwrap();
        cp.restore(&c1.commit).unwrap();
        assert_eq!(read(&f.root.join("a.txt")), "one\n");
        assert_eq!(read(&lib.join("f")), "v2\n");
        assert_eq!(read(&f.root.join("n/y")), "y2\n");
        assert_eq!(read(&f.root.join("n/z")), "z\n");
    }

    #[test]
    fn ignored_files_are_not_captured_and_left_alone() {
        let f = fixture();
        fs::write(f.root.join(".gitignore"), "*.log\n").unwrap();
        fs::write(f.root.join("out.log"), "log1\n").unwrap();
        let cp = Checkpoints::open(&f.store, &f.root, "s1").unwrap();
        let c1 = cp.capture("c1").unwrap().commit;
        assert_eq!(files(&cp, &c1), [".gitignore", "a.txt"]);

        assert!(cp.is_ignored(Path::new("out.log")));
        assert!(cp.is_ignored(&f.root.join("out.log")));
        assert!(cp.is_ignored(&f.root.join("never/written.log")));
        assert!(!cp.is_ignored(Path::new("a.txt")));
        assert!(!cp.is_ignored(&f.root.join("a.txt")));
        assert!(!cp.is_ignored(&f.base.join("elsewhere.log")));
        assert!(!cp.is_ignored(Path::new("../elsewhere.log")));

        fs::write(f.root.join("out.log"), "log2\n").unwrap();
        fs::write(f.root.join("a.txt"), "two\n").unwrap();
        cp.restore(&c1).unwrap();
        assert_eq!(read(&f.root.join("a.txt")), "one\n");
        assert_eq!(read(&f.root.join("out.log")), "log2\n");
    }

    #[test]
    fn paths_below_a_symlink_are_left_alone() {
        let f = fixture();
        let outside = f.base.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("f"), "outside\n").unwrap();
        for d in ["d", "lnk"] {
            fs::create_dir(f.root.join(d)).unwrap();
            fs::write(f.root.join(d).join("f"), "inside\n").unwrap();
        }
        let cp = Checkpoints::open(&f.store, &f.root, "s1").unwrap();
        let c1 = cp.capture("c1").unwrap().commit;

        // `d` becomes a captured symlink; `lnk` an ignored one.
        for d in ["d", "lnk"] {
            fs::remove_dir_all(f.root.join(d)).unwrap();
            symlink(&outside, f.root.join(d)).unwrap();
        }
        fs::write(f.root.join(".gitignore"), "/lnk\n").unwrap();
        cp.capture("c2").unwrap();

        let res = cp.restore(&c1).unwrap();
        assert_eq!(
            res.left_alone,
            [(PathBuf::from("lnk/f"), "below a symlink")]
        );
        assert_eq!(read(&outside.join("f")), "outside\n");
        assert!(fs::symlink_metadata(f.root.join("d")).unwrap().is_dir());
        assert_eq!(read(&f.root.join("d/f")), "inside\n");
        assert!(
            fs::symlink_metadata(f.root.join("lnk"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    /// Every path under `dir` with its size and modification time.
    fn listing(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in fs::read_dir(&d).unwrap() {
                let entry = entry.unwrap();
                let m = entry.metadata().unwrap();
                out.push(format!(
                    "{} {} {:?}",
                    entry.path().display(),
                    m.len(),
                    m.modified().ok()
                ));
                if m.is_dir() {
                    stack.push(entry.path());
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn project_git_dir_is_untouched() {
        let f = fixture();
        fs::write(f.root.join("dirty.txt"), "x\n").unwrap();
        let state = |root: &Path| {
            (
                git_in(root, &["rev-parse", "HEAD"]),
                git_in(root, &["--no-optional-locks", "status", "--porcelain"]),
            )
        };
        let before = (state(&f.root), listing(&f.root.join(".git")));

        let cp = Checkpoints::open(&f.store, &f.root, "s1").unwrap();
        let c1 = cp.capture("c1").unwrap().commit;
        fs::write(f.root.join("a.txt"), "two\n").unwrap();
        cp.capture("c2").unwrap();
        cp.is_ignored(Path::new("a.txt"));
        cp.diff(&c1, "HEAD").ok();
        cp.restore(&c1).unwrap();

        assert_eq!((state(&f.root), listing(&f.root.join(".git"))), before);
        assert!(!cp.git_dir().starts_with(cp.root()));
    }

    #[test]
    fn sessions_have_separate_refs_and_indexes() {
        let f = fixture();
        let s1 = Checkpoints::open(&f.store, &f.root, "s1").unwrap();
        let s2 = Checkpoints::open(&f.store, &f.root, "s2").unwrap();
        assert_eq!(s1.git_dir(), s2.git_dir());
        let a = s1.capture("s1").unwrap();
        fs::write(f.root.join("b.txt"), "b\n").unwrap();
        let b = s2.capture("s2").unwrap();
        assert!(b.changed, "a session's first capture always commits");
        assert_ne!(a.commit, b.commit);
        assert_eq!(s1.head().unwrap(), a.commit);
        assert_eq!(s2.head().unwrap(), b.commit);
        assert!(s1.capture("again").unwrap().changed);
        assert!(s1.git_dir().join("psi-index-s1").is_file());
        assert!(s1.git_dir().join("psi-index-s2").is_file());
        let refs = text(
            s1.run(
                "for-each-ref",
                ["for-each-ref", "--format=%(refname)"],
                None,
            )
            .unwrap(),
        );
        assert_eq!(refs, "refs/psi/s1\nrefs/psi/s2");
    }

    #[test]
    fn workspace_git_config_cannot_run_code() {
        let f = fixture();
        let marks = f.base.join("marks");
        fs::create_dir(&marks).unwrap();
        let trap = f.base.join("trap.sh");
        fs::write(
            &trap,
            format!(
                "#!/bin/sh\ntouch '{}/'\"$(basename \"$0\")\"\n",
                marks.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&trap, fs::Permissions::from_mode(0o755)).unwrap();
        let hooks = f.root.join(".git/hooks");
        fs::create_dir_all(&hooks).unwrap();
        for hook in [
            "pre-commit",
            "post-checkout",
            "post-index-change",
            "reference-transaction",
        ] {
            fs::copy(&trap, hooks.join(hook)).unwrap();
        }
        git_in(
            &f.root,
            &["config", "core.fsmonitor", trap.to_str().unwrap()],
        );
        let clean = format!("touch '{}/filter' && cat", marks.display());
        git_in(&f.root, &["config", "filter.evil.clean", &clean]);
        git_in(&f.root, &["config", "filter.evil.smudge", &clean]);
        fs::write(f.root.join(".gitattributes"), "* filter=evil\n").unwrap();
        fs::write(f.root.join("a.txt"), "two\n").unwrap();

        crate::git::project_root(&f.root);
        crate::git::git_dirs_outside(&f.root);
        let cp = Checkpoints::open(&f.store, &f.root, "s1").unwrap();
        let c1 = cp.capture("c1").unwrap().commit;
        fs::write(f.root.join("a.txt"), "three\n").unwrap();
        cp.capture("c2").unwrap();
        cp.is_ignored(Path::new("a.txt"));
        cp.restore(&c1).unwrap();
        assert_eq!(read(&f.root.join("a.txt")), "two\n");
        let fired: Vec<_> = fs::read_dir(&marks).unwrap().collect();
        assert!(fired.is_empty(), "{fired:?}");

        // The traps are armed: plain git in the workspace sets them off.
        let mut plain = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                plain.env_remove(key);
            }
        }
        plain
            .args(["add", "-A"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(&f.root)
            .output()
            .unwrap();
        let mut fired: Vec<_> = fs::read_dir(&marks)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        fired.sort();
        assert!(fired.contains(&"filter".to_string()), "{fired:?}");
        assert!(
            fired.contains(&"trap.sh".to_string()),
            "fsmonitor: {fired:?}"
        );
        assert!(
            fired.contains(&"post-index-change".to_string()),
            "{fired:?}"
        );
    }
}
