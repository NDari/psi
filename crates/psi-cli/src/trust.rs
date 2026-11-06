//! Project trust (psi-plan.md §9.3, pi parity): a project's own settings
//! and extensions (`<project>/.psi/`) load only after the user trusts them
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

/// The project's resource directories that need trust: what they hold
/// runs as the user or instructs the model (pi parity), including
/// sub-agent definitions (§10.1).
const TRUSTED_DIRS: &[&str] = &["extensions", "skills", "prompts", "themes", "agents"];

/// Files in `.psi` that need trust: settings, and MCP servers, whose
/// commands run as the user (§10.2).
const TRUSTED_FILES: &[&str] = &["settings.jsonc", "mcp.json"];

/// Most changes a prompt lists before "and N more".
const SHOWN_CHANGES: usize = 8;

/// Every file the loaders could reach: they read directories as they are,
/// so no ignore file hides one here, and links are followed as they follow
/// them (an ignored or linked extension still loads).
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in ignore::WalkBuilder::new(dir)
        .standard_filters(false)
        .follow_links(true)
        .build()
        .flatten()
    {
        if e.file_type().is_some_and(|t| t.is_file()) {
            out.push(e.into_path());
        }
    }
}

/// An extension's tests (`*_test.lua`) run only under `psi ext test`, so
/// changing them changes nothing a session loads (the cog trial's model
/// edited its extension tests often, and each edit asked again).
fn loads_in_a_session(path: &Path) -> bool {
    let in_extensions = path.components().any(|c| c.as_os_str() == "extensions");
    let test = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with("_test.lua"));
    !(in_extensions && test)
}

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

/// The files that need trust and their hashes: `.psi/settings.jsonc` and
/// `mcp.json`, extensions (not their tests), skills, prompts, themes and
/// agents, and `.agents/skills`. `None` when there are none (nothing to
/// trust).
pub fn snapshot(project: &Path) -> Option<BTreeMap<String, String>> {
    let dir = project.join(".psi");
    let mut files: Vec<PathBuf> = TRUSTED_FILES
        .iter()
        .map(|f| dir.join(f))
        .filter(|f| f.is_file())
        .collect();
    for d in TRUSTED_DIRS {
        files_under(&dir.join(d), &mut files);
    }
    let home = std::env::home_dir();
    for d in crate::resources::agents_dirs(project, home.as_deref()) {
        files_under(&d, &mut files);
    }
    let snap: BTreeMap<String, String> = files
        .iter()
        .filter(|f| loads_in_a_session(f))
        .map(|f| (key(project, f), sha256(f)))
        .collect();
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
/// .psi/extensions/x/init.lua"), at most a few; empty when nothing did.
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
/// next start or /reload. Only for a project already trusted as files, and
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
    if dir.join("mcp.json").is_file() {
        parts.push("MCP servers, which run as you".to_owned());
    }
    for d in ["skills", "prompts", "themes", "agents"] {
        if dir.join(d).is_dir() {
            parts.push(d.to_owned());
        }
    }
    if parts.is_empty() {
        return format!("{} has .agents/skills", project.display());
    }
    format!("{} has {}", dir.display(), parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_is_kept_until_a_file_changes_and_says_which() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let p = d.path().join("proj");
        std::fs::create_dir_all(p.join(".psi/extensions/x")).unwrap();
        assert_eq!(snapshot(&p), None, "nothing to trust");
        std::fs::write(p.join(".psi/extensions/a.lua"), "return {}").unwrap();
        std::fs::write(p.join(".psi/extensions/x/init.lua"), "return {}").unwrap();
        let f = snapshot(&p).unwrap();
        assert!(!is_trusted(&home, &p, &f));
        assert_eq!(changes(&home, &p, &f), ["not trusted before"]);
        trust(&home, &p, &f).unwrap();
        assert!(is_trusted(&home, &p, &f));
        assert!(changes(&home, &p, &f).is_empty());
        std::fs::write(p.join(".psi/extensions/a.lua"), "return { changed = true }").unwrap();
        std::fs::write(p.join(".psi/extensions/b.lua"), "return {}").unwrap();
        std::fs::remove_file(p.join(".psi/extensions/x/init.lua")).unwrap();
        let g = snapshot(&p).unwrap();
        assert!(!is_trusted(&home, &p, &g), "a change asks again");
        assert_eq!(
            changes(&home, &p, &g),
            [
                "changed .psi/extensions/a.lua",
                "new .psi/extensions/b.lua",
                "removed .psi/extensions/x/init.lua"
            ]
        );
        assert!(describe(&p).contains("extension(s)"));
    }

    /// The loaders don't read ignore files or skip links, so neither can
    /// hide a file from the fingerprint.
    #[test]
    fn ignore_files_and_links_dont_hide_what_loads() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let p = d.path().join("proj");
        std::fs::create_dir_all(p.join(".git")).unwrap();
        std::fs::create_dir_all(p.join(".psi/extensions")).unwrap();
        std::fs::write(p.join(".psi/.ignore"), "*\n").unwrap();
        std::fs::write(p.join(".gitignore"), "*.lua\n").unwrap();
        std::fs::write(p.join(".psi/extensions/evil.lua"), "return {}").unwrap();
        let f = snapshot(&p).expect("an ignored extension still needs trust");
        assert!(f.contains_key(".psi/extensions/evil.lua"), "{f:?}");
        // A linked extension: a change behind the link asks again.
        let elsewhere = d.path().join("tools/x");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("init.lua"), "return {}").unwrap();
        std::os::unix::fs::symlink(&elsewhere, p.join(".psi/extensions/x")).unwrap();
        let f = snapshot(&p).unwrap();
        assert!(f.contains_key(".psi/extensions/x/init.lua"), "{f:?}");
        trust(&home, &p, &f).unwrap();
        std::fs::write(elsewhere.join("init.lua"), "return { v = 2 }").unwrap();
        assert!(!is_trusted(&home, &p, &snapshot(&p).unwrap()));
    }

    #[test]
    fn extension_tests_dont_need_trust() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let p = d.path().join("proj");
        std::fs::create_dir_all(p.join(".psi/extensions/x")).unwrap();
        std::fs::write(p.join(".psi/extensions/x/init.lua"), "return {}").unwrap();
        std::fs::write(p.join(".psi/extensions/x/x_test.lua"), "a").unwrap();
        let f = snapshot(&p).unwrap();
        assert_eq!(f.len(), 1, "{f:?}");
        trust(&home, &p, &f).unwrap();
        std::fs::write(p.join(".psi/extensions/x/x_test.lua"), "b").unwrap();
        assert!(is_trusted(&home, &p, &snapshot(&p).unwrap()));
    }

    #[test]
    fn an_approved_write_keeps_the_trust() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let p = d.path().join("proj");
        let ext = p.join(".psi/extensions/x/init.lua");
        std::fs::create_dir_all(ext.parent().unwrap()).unwrap();
        std::fs::write(&ext, "return {}").unwrap();
        // Not trusted yet: an approved write doesn't trust the project.
        trust_file(&home, &p, &ext).unwrap();
        assert!(!is_trusted(&home, &p, &snapshot(&p).unwrap()));
        trust(&home, &p, &snapshot(&p).unwrap()).unwrap();
        std::fs::write(&ext, "return { v = 2 }").unwrap();
        trust_file(&home, &p, &ext).unwrap();
        assert!(is_trusted(&home, &p, &snapshot(&p).unwrap()));
        // A file changed some other way still asks.
        std::fs::write(p.join(".psi/settings.jsonc"), "{}").unwrap();
        let now = snapshot(&p).unwrap();
        assert!(!is_trusted(&home, &p, &now));
        assert_eq!(changes(&home, &p, &now), ["new .psi/settings.jsonc"]);
    }

    #[test]
    fn trust_from_an_older_psi_asks_once_more() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let p = d.path().join("proj");
        std::fs::create_dir_all(p.join(".psi")).unwrap();
        std::fs::write(p.join(".psi/settings.jsonc"), "{}").unwrap();
        std::fs::create_dir_all(home.join(".psi")).unwrap();
        let old = serde_json::json!({ p.display().to_string(): "08f209aaf57c16c9" });
        std::fs::write(home.join(".psi/trust.json"), old.to_string()).unwrap();
        let now = snapshot(&p).unwrap();
        assert!(!is_trusted(&home, &p, &now));
        assert!(changes(&home, &p, &now)[0].contains("older Psi"));
        trust(&home, &p, &now).unwrap();
        assert!(is_trusted(&home, &p, &now));
    }

    #[test]
    fn skills_need_trust_too() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("proj");
        std::fs::create_dir_all(p.join(".git")).unwrap();
        std::fs::create_dir_all(p.join(".psi/skills/x")).unwrap();
        std::fs::write(p.join(".psi/skills/x/SKILL.md"), "a").unwrap();
        let f = snapshot(&p).unwrap();
        assert!(
            describe(&p).ends_with(".psi has skills"),
            "{}",
            describe(&p)
        );
        std::fs::create_dir_all(p.join(".agents/skills/y")).unwrap();
        std::fs::write(p.join(".agents/skills/y/SKILL.md"), "b").unwrap();
        assert_ne!(snapshot(&p).unwrap(), f);
    }

    #[test]
    fn mcp_servers_and_agents_need_trust() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let p = d.path().join("proj");
        std::fs::create_dir_all(p.join(".psi")).unwrap();
        // Only an mcp.json: its commands run as the user, so there is
        // something to trust, and a change asks again.
        std::fs::write(p.join(".psi/mcp.json"), r#"{"mcpServers": {}}"#).unwrap();
        let f = snapshot(&p).expect("mcp.json needs trust");
        assert!(
            describe(&p).contains("MCP servers, which run as you"),
            "{}",
            describe(&p)
        );
        trust(&home, &p, &f).unwrap();
        std::fs::write(
            p.join(".psi/mcp.json"),
            r#"{"mcpServers": {"x": {"command": "sh"}}}"#,
        )
        .unwrap();
        assert!(!is_trusted(&home, &p, &snapshot(&p).unwrap()));
        // Only an agent file.
        let q = d.path().join("proj2");
        std::fs::create_dir_all(q.join(".psi/agents")).unwrap();
        std::fs::write(q.join(".psi/agents/a.md"), "---\ndescription: x\n---\nhi").unwrap();
        assert!(snapshot(&q).is_some());
        assert!(describe(&q).ends_with("agents"), "{}", describe(&q));
    }
}
