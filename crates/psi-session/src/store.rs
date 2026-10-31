//! Session files on disk (psi-plan.md §7.1):
//! `~/.psi/sessions/<cwd-slug>/<UTC time>_<id>.jsonl`, appended as a run goes,
//! with the wire log beside it as `<same name>.wire.jsonl`.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use crate::time::{now_ms, utc_stamp};
use crate::{Entry, EntryBody, IdGen, SessionError, read_entries};

#[derive(Debug)]
pub struct SessionFile {
    path: PathBuf,
    file: File,
    entries: Vec<Entry>,
    ids: IdGen,
    /// The entry new ones attach to.
    leaf: Option<String>,
}

/// The directory name for a working directory: its path with `/` as `-`.
pub fn cwd_slug(cwd: &Path) -> String {
    let s = cwd.display().to_string();
    let s = s.trim_matches('/').replace('/', "-");
    format!("-{s}-")
}

impl SessionFile {
    /// A new, empty session for `cwd` under `root` (`~/.psi/sessions`).
    pub fn create(root: &Path, cwd: &Path) -> Result<Self, SessionError> {
        let dir = root.join(cwd_slug(cwd));
        std::fs::create_dir_all(&dir)?;
        let u = uuid::Uuid::now_v7().simple().to_string();
        let name = format!("{}_{}.jsonl", utc_stamp(now_ms()), &u[u.len() - 8..]);
        let path = dir.join(name);
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            path,
            file,
            entries: Vec::new(),
            ids: IdGen::random_after(None),
            leaf: None,
        })
    }

    /// An existing session; new entries continue from its last one.
    pub fn open(path: &Path) -> Result<Self, SessionError> {
        let entries = read_entries(BufReader::new(File::open(path)?))?;
        let file = OpenOptions::new().append(true).open(path)?;
        let leaf = entries.last().map(|e| e.id.clone());
        Ok(Self {
            path: path.to_owned(),
            file,
            ids: IdGen::random_after(leaf.clone()),
            entries,
            leaf,
        })
    }

    /// The most recent session file for `cwd`, if any.
    pub fn latest(root: &Path, cwd: &Path) -> Option<PathBuf> {
        let dir = root.join(cwd_slug(cwd));
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "jsonl")
                    && !p.to_string_lossy().ends_with(".wire.jsonl")
            })
            .collect();
        files.sort();
        files.pop()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `<session>.wire.jsonl`.
    pub fn wire_path(&self) -> PathBuf {
        let s = self.path.to_string_lossy();
        PathBuf::from(format!("{}.wire.jsonl", s.trim_end_matches(".jsonl")))
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Appends an entry under the current leaf and writes it out at once.
    pub fn append(&mut self, body: EntryBody) -> Result<&Entry, SessionError> {
        let mut e = self.ids.push(now_ms(), body);
        e.parent_id = self.leaf.clone();
        let mut line = serde_json::to_vec(&e).map_err(std::io::Error::other)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.flush()?;
        self.leaf = Some(e.id.clone());
        self.entries.push(e);
        Ok(self
            .entries
            .last()
            .unwrap_or_else(|| unreachable!("just pushed")))
    }

    /// The entry new ones attach to.
    pub fn leaf(&self) -> Option<&str> {
        self.leaf.as_deref()
    }

    /// Moves the leaf to an existing entry (/rewind, /tree): the next entry
    /// starts a new branch there. Persisted only once an entry is appended.
    pub fn set_leaf(&mut self, id: &str) -> Result<(), SessionError> {
        if !self.entries.iter().any(|e| e.id == id) {
            return Err(SessionError::NoEntry(id.to_owned()));
        }
        self.leaf = Some(id.to_owned());
        Ok(())
    }

    /// The current branch, from the root to the leaf.
    pub fn branch(&self) -> Vec<&Entry> {
        branch(&self.entries, self.leaf.as_deref())
    }
}

/// The path from the root to `leaf`, following `parent_id`.
pub fn branch<'a>(entries: &'a [Entry], leaf: Option<&str>) -> Vec<&'a Entry> {
    let mut out = Vec::new();
    let mut cur = leaf;
    while let Some(id) = cur {
        let Some(e) = entries.iter().rev().find(|e| e.id == id) else {
            break;
        };
        out.push(e);
        cur = e.parent_id.as_deref();
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(c: &str) -> EntryBody {
        EntryBody::checkpoint(c)
    }

    #[test]
    fn create_append_reopen_continue() {
        let root = tempfile::tempdir().unwrap();
        let cwd = Path::new("/home/u/proj");
        let mut s = SessionFile::create(root.path(), cwd).unwrap();
        assert!(s.path().starts_with(root.path().join("-home-u-proj-")));
        assert!(s.wire_path().to_string_lossy().ends_with(".wire.jsonl"));
        let a = s.append(cp("a")).unwrap().id.clone();
        s.append(cp("b")).unwrap();
        assert_eq!(
            SessionFile::latest(root.path(), cwd).as_deref(),
            Some(s.path())
        );

        let mut again = SessionFile::open(s.path()).unwrap();
        assert_eq!(again.entries().len(), 2);
        let c = again.append(cp("c")).unwrap().clone();
        assert_eq!(c.parent_id.as_deref(), Some(again.entries()[1].id.as_str()));
        let ids: Vec<&str> = again.branch().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0], a);
        assert_eq!(ids[0].len(), 8);
    }

    #[test]
    fn branch_follows_parents_not_file_order() {
        let mut ids = IdGen::new();
        let a = ids.push(0, cp("a"));
        let b = ids.push(0, cp("b"));
        let mut c = ids.push(0, cp("c"));
        c.parent_id = Some(a.id.clone()); // a fork from a
        let all = vec![a.clone(), b, c.clone()];
        let got: Vec<&str> = branch(&all, Some(&c.id))
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(got, [a.id.as_str(), c.id.as_str()]);
    }
}
