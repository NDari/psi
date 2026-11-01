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

/// The project's resource directories that need trust: what they hold
/// runs as the user or instructs the model (pi parity), including
/// sub-agent definitions (§10.1).
const TRUSTED_DIRS: &[&str] = &["extensions", "skills", "prompts", "themes", "agents"];

/// Files in `.psi` that need trust: settings, and MCP servers, whose
/// commands run as the user (§10.2).
const TRUSTED_FILES: &[&str] = &["settings.jsonc", "mcp.json"];

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in ignore::WalkBuilder::new(dir)
        .hidden(false)
        .build()
        .flatten()
    {
        if e.file_type().is_some_and(|t| t.is_file()) {
            out.push(e.into_path());
        }
    }
}

/// A fingerprint of the project's `.psi/settings.jsonc` and `mcp.json`,
/// its extensions, skills, prompts, themes and agents, and its
/// `.agents/skills`, or `None` when it has none of them (nothing to trust).
pub fn fingerprint(project: &Path) -> Option<String> {
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

    #[test]
    fn skills_need_trust_too() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("proj");
        std::fs::create_dir_all(p.join(".git")).unwrap();
        std::fs::create_dir_all(p.join(".psi/skills/x")).unwrap();
        std::fs::write(p.join(".psi/skills/x/SKILL.md"), "a").unwrap();
        let f = fingerprint(&p).unwrap();
        assert!(
            describe(&p).ends_with(".psi has skills"),
            "{}",
            describe(&p)
        );
        std::fs::create_dir_all(p.join(".agents/skills/y")).unwrap();
        std::fs::write(p.join(".agents/skills/y/SKILL.md"), "b").unwrap();
        assert_ne!(fingerprint(&p).unwrap(), f);
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
        let f = fingerprint(&p).expect("mcp.json needs trust");
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
        assert!(!is_trusted(&home, &p, &fingerprint(&p).unwrap()));
        // Only an agent file.
        let q = d.path().join("proj2");
        std::fs::create_dir_all(q.join(".psi/agents")).unwrap();
        std::fs::write(q.join(".psi/agents/a.md"), "---\ndescription: x\n---\nhi").unwrap();
        assert!(fingerprint(&q).is_some());
        assert!(describe(&q).ends_with("agents"), "{}", describe(&q));
    }
}
