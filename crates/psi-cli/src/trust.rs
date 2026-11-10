//! Project trust (psi-plan.md §9.3, pi parity): a project's own settings
//! (`<project>/.psi/settings.jsonc`) load only after the user trusts them
//! once, and again whenever they change on disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// What the user trusted, per project: `~/.psi/trust.json`, each file that
/// needs trust with its SHA-256. (Until 2026-11-04 one hash of them all,
/// from Rust's `DefaultHasher`, which may change between Rust releases and
/// can't say which file changed.)
fn store(home: &Path) -> PathBuf {
    home.join(".psi/trust.json")
}

/// Files in `.psi` that need trust.
const TRUSTED_FILES: &[&str] = &["settings.jsonc"];

/// Most changes a prompt lists before "and N more".
const SHOWN_CHANGES: usize = 8;

/// How a file is named in the store and the prompt: relative to the project
/// when inside it.
fn key(project: &Path, file: &Path) -> String {
    file.strip_prefix(project)
        .unwrap_or(file)
        .display()
        .to_string()
}

fn sha256(file: &Path) -> String {
    let bytes = std::fs::read(file).unwrap_or_default();
    Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The files that need trust and their hashes: `.psi/settings.jsonc`.
/// `None` when there are none (nothing to trust).
pub fn snapshot(project: &Path) -> Option<BTreeMap<String, String>> {
    let dir = project.join(".psi");
    let files: Vec<PathBuf> = TRUSTED_FILES
        .iter()
        .map(|f| dir.join(f))
        .filter(|f| f.is_file())
        .collect();
    let snap: BTreeMap<String, String> =
        files.iter().map(|f| (key(project, f), sha256(f))).collect();
    (!snap.is_empty()).then_some(snap)
}

fn load(home: &Path) -> serde_json::Map<String, Value> {
    std::fs::read_to_string(store(home))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(home: &Path, all: &serde_json::Map<String, Value>) -> Result<()> {
    let path = store(home);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).with_context(|| format!("creating {}", p.display()))?;
    }
    let text = serde_json::to_string_pretty(all)?;
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

/// What was trusted for `project`: `None` never; `Some(None)` by an older
/// Psi, which kept one hash and can't be compared file by file.
fn trusted(home: &Path, project: &Path) -> Option<Option<BTreeMap<String, String>>> {
    let all = load(home);
    let v = all.get(&project.display().to_string())?;
    Some(serde_json::from_value(v.clone()).ok())
}

/// What changed since the user trusted the project, one line each ("changed
/// .psi/settings.jsonc"), at most a few; empty when nothing did.
pub fn changes(home: &Path, project: &Path, now: &BTreeMap<String, String>) -> Vec<String> {
    let before = match trusted(home, project) {
        None => return vec!["not trusted before".into()],
        Some(None) => return vec!["trusted by an older Psi, which can't say what changed".into()],
        Some(Some(b)) => b,
    };
    let mut out: Vec<String> = Vec::new();
    for (f, h) in now {
        match before.get(f) {
            None => out.push(format!("new {f}")),
            Some(old) if old != h => out.push(format!("changed {f}")),
            Some(_) => {}
        }
    }
    for f in before.keys().filter(|f| !now.contains_key(*f)) {
        out.push(format!("removed {f}"));
    }
    if out.len() > SHOWN_CHANGES {
        let more = out.len() - SHOWN_CHANGES;
        out.truncate(SHOWN_CHANGES);
        out.push(format!("and {more} more"));
    }
    out
}

pub fn is_trusted(home: &Path, project: &Path, now: &BTreeMap<String, String>) -> bool {
    matches!(trusted(home, project), Some(Some(b)) if &b == now)
}

pub fn trust(home: &Path, project: &Path, now: &BTreeMap<String, String>) -> Result<()> {
    let mut all = load(home);
    all.insert(project.display().to_string(), serde_json::to_value(now)?);
    save(home, &all)
}

/// After the user approved a write to `file` through Psi's file tools:
/// record its new hash, so a change they approved doesn't ask again at the
/// next start. Only for a project already trusted as files, and
/// a file that needs trust; changes from bash or outside Psi still ask.
pub fn trust_file(home: &Path, project: &Path, file: &Path) -> Result<()> {
    let Some(Some(mut files)) = trusted(home, project) else {
        return Ok(());
    };
    let needs_trust = snapshot(project).is_some_and(|s| s.contains_key(&key(project, file)));
    if !needs_trust {
        return Ok(());
    }
    files.insert(key(project, file), sha256(file));
    trust(home, project, &files)
}

/// What there is to trust, for the prompt.
pub fn describe(project: &Path) -> String {
    format!("{} has settings", project.join(".psi").display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_is_kept_until_the_settings_change() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let p = d.path().join("proj");
        std::fs::create_dir_all(p.join(".psi")).unwrap();
        assert_eq!(snapshot(&p), None, "nothing to trust");
        std::fs::write(p.join(".psi/settings.jsonc"), "{}").unwrap();
        let f = snapshot(&p).unwrap();
        assert!(!is_trusted(&home, &p, &f));
        assert_eq!(changes(&home, &p, &f), ["not trusted before"]);
        trust(&home, &p, &f).unwrap();
        assert!(is_trusted(&home, &p, &f));
        assert!(changes(&home, &p, &f).is_empty());
        std::fs::write(p.join(".psi/settings.jsonc"), "{ \"mode\": \"auto\" }").unwrap();
        let g = snapshot(&p).unwrap();
        assert!(!is_trusted(&home, &p, &g), "a change asks again");
        assert_eq!(changes(&home, &p, &g), ["changed .psi/settings.jsonc"]);
        assert!(describe(&p).ends_with(".psi has settings"));
    }

    #[test]
    fn an_approved_write_keeps_the_trust() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let p = d.path().join("proj");
        let file = p.join(".psi/settings.jsonc");
        std::fs::create_dir_all(p.join(".psi")).unwrap();
        std::fs::write(&file, "{}").unwrap();
        // Not trusted yet: an approved write doesn't trust the project.
        trust_file(&home, &p, &file).unwrap();
        assert!(!is_trusted(&home, &p, &snapshot(&p).unwrap()));
        trust(&home, &p, &snapshot(&p).unwrap()).unwrap();
        std::fs::write(&file, "{ \"mode\": \"auto\" }").unwrap();
        trust_file(&home, &p, &file).unwrap();
        assert!(is_trusted(&home, &p, &snapshot(&p).unwrap()));
        // Changed some other way, it still asks.
        std::fs::write(&file, "{ \"mode\": \"strict\" }").unwrap();
        assert!(!is_trusted(&home, &p, &snapshot(&p).unwrap()));
    }
}
