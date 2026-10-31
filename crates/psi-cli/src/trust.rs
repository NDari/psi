//! Project trust (psi-plan.md §9.3, pi parity): a project's own settings
//! and extensions (`<project>/.psi/`) load only after the user trusts them
//! once, and again whenever they change on disk.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// What the user trusted, per project: `~/.psi/trust.json`.
fn store(home: &Path) -> PathBuf {
    home.join(".psi/trust.json")
}

/// A fingerprint of the project's `.psi/settings.jsonc` and extensions,
/// or `None` when it has neither (nothing to trust).
pub fn fingerprint(project: &Path) -> Option<String> {
    let dir = project.join(".psi");
    let mut files: Vec<PathBuf> = Vec::new();
    if dir.join("settings.jsonc").is_file() {
        files.push(dir.join("settings.jsonc"));
    }
    for e in ignore::WalkBuilder::new(dir.join("extensions"))
        .hidden(false)
        .build()
        .flatten()
    {
        if e.file_type().is_some_and(|t| t.is_file()) {
            files.push(e.into_path());
        }
    }
    if files.is_empty() {
        return None;
    }
    files.sort();
    let mut h = DefaultHasher::new();
    for f in files {
        f.hash(&mut h);
        std::fs::read(&f).unwrap_or_default().hash(&mut h);
    }
    Some(format!("{:016x}", h.finish()))
}

fn load(home: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(store(home))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn is_trusted(home: &Path, project: &Path, print: &str) -> bool {
    load(home)
        .get(&project.display().to_string())
        .map(String::as_str)
        == Some(print)
}

pub fn trust(home: &Path, project: &Path, print: &str) -> Result<()> {
    let mut all = load(home);
    all.insert(project.display().to_string(), print.to_owned());
    let path = store(home);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).with_context(|| format!("creating {}", p.display()))?;
    }
    let text = serde_json::to_string_pretty(&all)?;
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

/// What there is to trust, for the prompt.
pub fn describe(project: &Path) -> String {
    let dir = project.join(".psi");
    let mut parts = Vec::new();
    if dir.join("settings.jsonc").is_file() {
        parts.push("settings".to_owned());
    }
    let n = std::fs::read_dir(dir.join("extensions"))
        .map(|d| d.flatten().count())
        .unwrap_or(0);
    if n > 0 {
        parts.push(format!("{n} extension(s), which run as you"));
    }
    format!("{} has {}", dir.display(), parts.join(" and "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_is_kept_until_the_files_change() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let p = d.path().join("proj");
        std::fs::create_dir_all(p.join(".psi/extensions")).unwrap();
        assert_eq!(fingerprint(&p), None, "nothing to trust");
        std::fs::write(p.join(".psi/extensions/a.lua"), "return {}").unwrap();
        let f = fingerprint(&p).unwrap();
        assert!(!is_trusted(&home, &p, &f));
        trust(&home, &p, &f).unwrap();
        assert!(is_trusted(&home, &p, &f));
        std::fs::write(p.join(".psi/extensions/a.lua"), "return { changed = true }").unwrap();
        let g = fingerprint(&p).unwrap();
        assert_ne!(f, g);
        assert!(!is_trusted(&home, &p, &g), "a change asks again");
        assert!(describe(&p).contains("1 extension(s)"));
    }
}
