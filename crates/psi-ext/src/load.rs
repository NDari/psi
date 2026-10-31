//! Finding extensions (psi-plan.md §9.3): `name.lua`, a directory with
//! `init.lua`, or a script tool (a directory with `tool.json`), in
//! `~/.psi/extensions/`, the trusted project's `.psi/extensions/`, and
//! `--extension <path>`.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value as Json;

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The Lua file to run (`name.lua`, or `<dir>/init.lua`).
    Lua(PathBuf),
    Script(ScriptTool),
}

impl Source {
    pub fn path(&self) -> &Path {
        match self {
            Source::Lua(p) => p,
            Source::Script(s) => &s.dir,
        }
    }
}

/// A script tool (D-22): an executable that gets the arguments as JSON on
/// stdin and prints its result; it runs inside the bash boundary.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScriptTool {
    #[serde(skip)]
    pub dir: PathBuf,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub params: serde_json::Map<String, Json>,
    #[serde(default)]
    pub required: Option<Vec<String>>,
    #[serde(default)]
    pub always_ask: bool,
    #[serde(default)]
    pub read_only: bool,
    /// A path relative to the directory, or a program and its arguments.
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Command {
    One(String),
    Argv(Vec<String>),
}

impl ScriptTool {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("tool.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut t: ScriptTool =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        t.dir = dir.to_owned();
        Ok(t)
    }

    /// The program and arguments to run, resolved against the directory.
    pub fn argv(&self) -> Vec<String> {
        let mut v = match &self.command {
            Command::One(c) => vec![c.clone()],
            Command::Argv(a) => a.clone(),
        };
        if let Some(first) = v.first_mut()
            && (first.contains('/') || self.dir.join(&*first).is_file())
        {
            *first = self.dir.join(&*first).display().to_string();
        }
        v
    }
}

/// One extension at `path`, if it is one.
pub fn source(path: &Path) -> Option<Result<Source, String>> {
    if path.is_file() && path.extension().is_some_and(|e| e == "lua") {
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with("_test.lua"))
        {
            return None;
        }
        return Some(Ok(Source::Lua(path.to_owned())));
    }
    if path.is_dir() {
        if path.join("init.lua").is_file() {
            return Some(Ok(Source::Lua(path.join("init.lua"))));
        }
        if path.join("tool.json").is_file() {
            return Some(ScriptTool::load(path).map(Source::Script));
        }
    }
    None
}

/// Every extension in a directory, in name order.
pub fn discover(dir: &Path) -> Vec<Result<Source, String>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths.iter().filter_map(|p| source(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_files_dirs_and_script_tools() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.lua"), "return {}").unwrap();
        std::fs::write(d.path().join("a_test.lua"), "").unwrap();
        std::fs::create_dir_all(d.path().join("b")).unwrap();
        std::fs::write(d.path().join("b/init.lua"), "return {}").unwrap();
        std::fs::create_dir_all(d.path().join("c")).unwrap();
        std::fs::write(
            d.path().join("c/tool.json"),
            r#"{"name": "word_count", "description": "Count words", "params": {"path": {"type": "string", "description": "A file"}}, "command": "run.sh"}"#,
        )
        .unwrap();
        std::fs::write(d.path().join("c/run.sh"), "").unwrap();
        std::fs::write(d.path().join("notes.txt"), "").unwrap();
        let found: Vec<Source> = discover(d.path()).into_iter().map(|r| r.unwrap()).collect();
        assert_eq!(found.len(), 3);
        assert_eq!(found[0], Source::Lua(d.path().join("a.lua")));
        assert_eq!(found[1], Source::Lua(d.path().join("b/init.lua")));
        let Source::Script(t) = &found[2] else {
            panic!()
        };
        assert_eq!(t.name, "word_count");
        assert_eq!(t.argv(), [d.path().join("c/run.sh").display().to_string()]);
    }
}
