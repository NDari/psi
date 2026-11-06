//! Packages (psi-plan.md §10, pi parity, G-04): a local directory or a git
//! repository holding extensions, skills, prompts and themes. `psi install`
//! clones git sources under `~/.psi/git/` (or the project's `.psi/git/` with
//! `-l`) and records the source in the settings' `packages` list; at startup
//! each package's resources load after the user's own.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::CstRootNode;
use serde_json::Value;

/// Where a package comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Local(PathBuf),
    Git(GitSource),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSource {
    /// What `git clone` gets.
    pub repo: String,
    pub host: String,
    /// `user/repo`, without `.git`.
    pub path: String,
    /// A pinned branch, tag or commit (`@ref`).
    pub git_ref: Option<String>,
}

impl Source {
    /// pi's rules: `git:` takes any git form (`git:host/user/repo`,
    /// `git:git@host:user/repo`); `https://`, `ssh://` and `git://` URLs
    /// are git; anything else is a local path. `@ref` pins a git source.
    pub fn parse(s: &str, base: &Path, home: Option<&Path>) -> Result<Source> {
        let s = s.trim();
        let url = ["https://", "http://", "ssh://", "git://"]
            .iter()
            .any(|p| s.starts_with(p));
        if let Some(g) = s.strip_prefix("git:") {
            return git(g).map(Source::Git);
        }
        if url {
            return git(s).map(Source::Git);
        }
        let p = match (s.strip_prefix("~/"), home) {
            (Some(rest), Some(h)) => h.join(rest),
            _ => base.join(s),
        };
        Ok(Source::Local(normalize(&p)))
    }

    /// Two specs are the same package when this matches: the git host
    /// and path (any protocol, any ref), or the local path.
    pub fn identity(&self) -> String {
        match self {
            Source::Local(p) => format!("local:{}", p.display()),
            Source::Git(g) => format!("git:{}/{}", g.host, g.path),
        }
    }

    /// Where the package's files are.
    pub fn dir(&self, git_root: &Path) -> PathBuf {
        match self {
            Source::Local(p) => p.clone(),
            Source::Git(g) => git_root.join(&g.host).join(&g.path),
        }
    }
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn git(s: &str) -> Result<GitSource> {
    let bad = || anyhow::anyhow!("'{s}' is not a git source like git:github.com/user/repo[@ref]");
    if s.contains('\0') || s.contains('\\') {
        return Err(bad());
    }
    // (repo for clone, host, path with an optional @ref)
    let (repo_prefix, host, rest) = if let Some(after) = s.split_once("://").map(|(_, a)| a) {
        let scheme = &s[..s.find("://").unwrap_or(0) + 3];
        let (auth_host, path) = after.split_once('/').ok_or_else(bad)?;
        let host = auth_host.rsplit('@').next().unwrap_or(auth_host);
        (
            format!("{scheme}{auth_host}/"),
            host.to_owned(),
            path.to_owned(),
        )
    } else if let Some((user_host, path)) = s.split_once(':').filter(|(h, _)| h.contains('@')) {
        // scp form: git@host:user/repo
        let host = user_host.rsplit('@').next().unwrap_or(user_host);
        (format!("{user_host}:"), host.to_owned(), path.to_owned())
    } else {
        let (host, path) = s.split_once('/').ok_or_else(bad)?;
        if !host.contains('.') && host != "localhost" {
            return Err(bad());
        }
        (format!("https://{host}/"), host.to_owned(), path.to_owned())
    };
    let (path, git_ref) = match rest.rsplit_once('@') {
        Some((p, r)) if !r.is_empty() && !r.contains('/') => (p.to_owned(), Some(r.to_owned())),
        _ => (rest, None),
    };
    let path = path.trim_matches('/').trim_end_matches(".git").to_owned();
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() < 2
        || segments
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == ".." || p.starts_with('-'))
        || host.is_empty()
        || host == "."
        || host == ".."
        || host.contains('/')
    {
        return Err(bad());
    }
    // Git would take a leading `-` for an option (`-u<cmd>` is --upload-pack).
    if repo_prefix.starts_with('-') || git_ref.as_deref().is_some_and(|r| r.starts_with('-')) {
        return Err(bad());
    }
    Ok(GitSource {
        repo: format!("{repo_prefix}{path}"),
        host,
        path,
        git_ref,
    })
}

/// User or project settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    User,
    Project,
}

pub struct Paths {
    pub home: PathBuf,
    pub cwd: PathBuf,
}

impl Paths {
    pub fn settings(&self, scope: Scope) -> PathBuf {
        match scope {
            Scope::User => self.home.join(".psi/settings.jsonc"),
            Scope::Project => self.cwd.join(".psi/settings.jsonc"),
        }
    }

    pub fn git_root(&self, scope: Scope) -> PathBuf {
        match scope {
            Scope::User => self.home.join(".psi/git"),
            Scope::Project => self.cwd.join(".psi/git"),
        }
    }

    /// Relative local paths in a settings file are taken from the
    /// directory that holds it.
    fn base(&self, scope: Scope) -> PathBuf {
        match scope {
            Scope::User => self.home.join(".psi"),
            Scope::Project => self.cwd.join(".psi"),
        }
    }
}

/// The `packages` list of one settings file, as written.
pub fn listed(paths: &Paths, scope: Scope) -> Result<Vec<String>> {
    let path = paths.settings(scope);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Ok(Vec::new()),
    };
    let v = crate::config::parse_jsonc(&text, &path.display().to_string())?;
    Ok(entries(&v["packages"]))
}

/// Entries are a source string, or `{ "source": ... }` (pi's filter form;
/// Psi reads only the source).
fn entries(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e.as_str().or_else(|| e["source"].as_str()))
        .map(str::to_owned)
        .collect()
}

fn git_cmd(args: &[&str], dir: Option<&Path>) -> Result<()> {
    let mut c = Command::new("git");
    c.args(args);
    if let Some(d) = dir {
        c.current_dir(d);
    }
    let out = c.output().context("running git (is it installed?)")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Clones a git package, or brings an existing clone to its ref (or the
/// remote branch when unpinned).
fn fetch(g: &GitSource, dir: &Path) -> Result<()> {
    if dir.join(".git").exists() {
        match &g.git_ref {
            Some(r) => {
                git_cmd(&["fetch", "--quiet", "--tags", "origin"], Some(dir))?;
                git_cmd(&["checkout", "--quiet", "--detach", r], Some(dir))
            }
            None => git_cmd(&["pull", "--quiet", "--ff-only"], Some(dir)),
        }
    } else {
        if let Some(p) = dir.parent() {
            std::fs::create_dir_all(p).with_context(|| format!("creating {}", p.display()))?;
        }
        // Clean up after a failure only what this clone created.
        let existed = dir.exists();
        let undo = || {
            if !existed {
                let _ = std::fs::remove_dir_all(dir);
            }
        };
        let shown = dir.display().to_string();
        if let Err(e) = git_cmd(&["clone", "--quiet", "--", &g.repo, &shown], None) {
            undo();
            return Err(e);
        }
        if let Some(r) = &g.git_ref
            && let Err(e) = git_cmd(&["checkout", "--quiet", "--detach", r], Some(dir))
        {
            undo();
            return Err(e);
        }
        Ok(())
    }
}

/// Rewrites one settings file's `packages` list, keeping its comments.
fn edit_packages(path: &Path, f: impl FnOnce(&mut Vec<String>) -> bool) -> Result<bool> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let text = if text.trim().is_empty() {
        "{}\n".to_owned()
    } else {
        text
    };
    let opts = ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        ..ParseOptions::default()
    };
    let root =
        CstRootNode::parse(&text, &opts).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let current = crate::config::parse_jsonc(&text, &path.display().to_string())?;
    let mut list = entries(&current["packages"]);
    if !f(&mut list) {
        return Ok(false);
    }
    let obj = root.object_value_or_set();
    let arr = obj.array_value_or_set("packages");
    for e in arr.elements() {
        e.remove();
    }
    for s in &list {
        arr.append(s.as_str().into());
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).with_context(|| format!("creating {}", p.display()))?;
    }
    std::fs::write(path, root.to_string())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

/// `psi install`: fetch the package, then list it in the settings.
pub fn install(paths: &Paths, scope: Scope, spec: &str) -> Result<String> {
    let src = Source::parse(spec, &paths.cwd, Some(&paths.home))?;
    let dir = src.dir(&paths.git_root(scope));
    match &src {
        Source::Local(p) if !p.is_dir() => bail!("{} is not a directory", p.display()),
        Source::Local(_) => {}
        Source::Git(g) => {
            let root = paths.git_root(scope);
            std::fs::create_dir_all(&root)
                .with_context(|| format!("creating {}", root.display()))?;
            // Clones never belong in the project's own repository.
            let ignore = root.join(".gitignore");
            if !ignore.exists() {
                let _ = std::fs::write(&ignore, "*\n!.gitignore\n");
            }
            fetch(g, &dir)?;
        }
    }
    // Local paths are kept absolute, so they mean the same from any layer.
    let entry = match &src {
        Source::Local(p) => p.display().to_string(),
        Source::Git(_) => spec.trim().to_owned(),
    };
    let id = src.identity();
    let base = paths.base(scope);
    let home = paths.home.clone();
    edit_packages(&paths.settings(scope), |list| {
        let same =
            |e: &String| Source::parse(e, &base, Some(&home)).is_ok_and(|s| s.identity() == id);
        match list.iter().position(same) {
            Some(i) if list[i] == entry => false,
            Some(i) => {
                list[i] = entry.clone();
                true
            }
            None => {
                list.push(entry.clone());
                true
            }
        }
    })?;
    Ok(format!("installed {} in {}", entry, dir.display()))
}

/// `psi remove`: drop it from the settings, and delete a git clone.
pub fn remove(paths: &Paths, scope: Scope, spec: &str) -> Result<String> {
    let src = Source::parse(spec, &paths.cwd, Some(&paths.home))?;
    let id = src.identity();
    let base = paths.base(scope);
    let home = paths.home.clone();
    let removed = edit_packages(&paths.settings(scope), |list| {
        let n = list.len();
        list.retain(|e| !Source::parse(e, &base, Some(&home)).is_ok_and(|s| s.identity() == id));
        list.len() != n
    })?;
    if !removed {
        bail!("no package {spec} in {}", paths.settings(scope).display());
    }
    if let Source::Git(_) = &src {
        let dir = src.dir(&paths.git_root(scope));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
        }
    }
    Ok(format!("removed {spec}"))
}

/// `psi update`: fetch every git package (or one), pinned ones to their ref.
/// The project's packages only when the project is trusted (§9.3): its
/// settings are anyone's who can commit to the repository.
pub fn update(paths: &Paths, only: Option<&str>, project_trusted: bool) -> Result<Vec<String>> {
    let want = only
        .map(|s| Source::parse(s, &paths.cwd, Some(&paths.home)).map(|s| s.identity()))
        .transpose()?;
    let mut done = Vec::new();
    for scope in [Scope::User, Scope::Project] {
        if scope == Scope::Project && !project_trusted {
            if !listed(paths, scope).unwrap_or_default().is_empty() {
                done.push(
                    "skipped the project's packages: the project isn't trusted; start psi here to trust it"
                        .to_owned(),
                );
            }
            continue;
        }
        for e in listed(paths, scope)? {
            let src = Source::parse(&e, &paths.base(scope), Some(&paths.home))?;
            if want.as_ref().is_some_and(|w| *w != src.identity()) {
                continue;
            }
            if let Source::Git(g) = &src {
                fetch(g, &src.dir(&paths.git_root(scope)))?;
                done.push(format!("updated {e}"));
            }
        }
    }
    if let Some(s) = only
        && done.is_empty()
    {
        bail!("no git package {s} in the settings");
    }
    Ok(done)
}

/// What a package holds: directories of each kind of resource.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Contents {
    /// Directories of extensions, and single extensions.
    pub extension_dirs: Vec<PathBuf>,
    pub extensions: Vec<PathBuf>,
    pub skills: Vec<PathBuf>,
    pub prompts: Vec<PathBuf>,
    pub themes: Vec<PathBuf>,
}

/// A package's resources: from `package.json`'s `psi` key when it lists
/// them (or its `pi` key for skills, prompts and themes), else from the
/// conventional `extensions/`, `skills/`, `prompts/` and `themes/`. A
/// directory that is itself one extension is that extension.
pub fn contents(dir: &Path) -> Contents {
    let mut c = Contents::default();
    let manifest: Value = std::fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let list = |key: &str, field: &str| -> Option<Vec<PathBuf>> {
        let a = manifest[key][field].as_array()?;
        Some(
            a.iter()
                .filter_map(Value::as_str)
                .map(|p| normalize(&dir.join(p)))
                .filter(|p| p.starts_with(normalize(dir)))
                .collect(),
        )
    };
    let lists = ["extensions", "skills", "prompts", "themes"];
    let has_psi = lists.iter().any(|f| manifest["psi"][f].is_array());
    let has_pi = ["skills", "prompts", "themes"]
        .iter()
        .any(|f| manifest["pi"][f].is_array());
    if has_psi || has_pi {
        let key = if has_psi { "psi" } else { "pi" };
        c.extensions = if has_psi {
            list("psi", "extensions").unwrap_or_default()
        } else {
            Vec::new()
        };
        c.skills = list(key, "skills").unwrap_or_default();
        c.prompts = list(key, "prompts").unwrap_or_default();
        c.themes = list(key, "themes").unwrap_or_default();
        return c;
    }
    if dir.join("init.lua").is_file() || dir.join("tool.json").is_file() {
        c.extensions.push(dir.to_owned());
        return c;
    }
    let sub = |name: &str| Some(dir.join(name)).filter(|p| p.is_dir());
    c.extension_dirs.extend(sub("extensions"));
    c.skills.extend(sub("skills"));
    c.prompts.extend(sub("prompts"));
    c.themes.extend(sub("themes"));
    c
}

/// Every listed package's contents, the project's first (when trusted),
/// and problems such as a git package that isn't cloned.
pub fn installed(paths: &Paths, trusted: bool) -> (Vec<Contents>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let scopes: &[Scope] = if trusted {
        &[Scope::Project, Scope::User]
    } else {
        &[Scope::User]
    };
    let mut seen = Vec::new();
    for &scope in scopes {
        let list = match listed(paths, scope) {
            Ok(l) => l,
            Err(e) => {
                problems.push(format!("{e:#}"));
                continue;
            }
        };
        for e in list {
            let src = match Source::parse(&e, &paths.base(scope), Some(&paths.home)) {
                Ok(s) => s,
                Err(err) => {
                    problems.push(format!("package {e}: {err:#}"));
                    continue;
                }
            };
            // The project's entry for a package wins over the user's.
            if seen.contains(&src.identity()) {
                continue;
            }
            seen.push(src.identity());
            let dir = src.dir(&paths.git_root(scope));
            if !dir.is_dir() {
                let fix = match scope {
                    Scope::User => format!("psi install {e}"),
                    Scope::Project => format!("psi install -l {e}"),
                };
                problems.push(format!(
                    "package {e} is not at {} (run {fix})",
                    dir.display()
                ));
                continue;
            }
            out.push(contents(&dir));
        }
    }
    (out, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(s: &str) -> GitSource {
        match Source::parse(s, Path::new("/w"), None).unwrap() {
            Source::Git(g) => g,
            other => panic!("{s}: {other:?}"),
        }
    }

    #[test]
    fn git_source_forms() {
        let a = g("https://github.com/user/repo");
        assert_eq!(
            (a.repo.as_str(), a.host.as_str(), a.path.as_str()),
            ("https://github.com/user/repo", "github.com", "user/repo")
        );
        let b = g("https://github.com/user/repo.git@v1.0.0");
        assert_eq!(
            (b.path.as_str(), b.git_ref.as_deref()),
            ("user/repo", Some("v1.0.0"))
        );
        assert_eq!(b.repo, "https://github.com/user/repo");
        let c = g("git:git@github.com:user/repo@main");
        assert_eq!(
            (c.repo.as_str(), c.host.as_str(), c.git_ref.as_deref()),
            ("git@github.com:user/repo", "github.com", Some("main"))
        );
        let d = g("git:github.com/user/repo");
        assert_eq!(d.repo, "https://github.com/user/repo");
        let e = g("ssh://git@github.com/user/repo");
        assert_eq!(
            (e.repo.as_str(), e.host.as_str()),
            ("ssh://git@github.com/user/repo", "github.com")
        );
        // All the same package.
        let ids: Vec<String> = [
            "https://github.com/user/repo",
            "git:git@github.com:user/repo@v2",
            "ssh://git@github.com/user/repo.git",
        ]
        .iter()
        .map(|s| Source::parse(s, Path::new("/"), None).unwrap().identity())
        .collect();
        assert!(
            ids.iter().all(|i| i == "git:github.com/user/repo"),
            "{ids:?}"
        );
        for bad in [
            "git:user/repo",
            "git:github.com/only",
            "https://h.com/a/../b",
            // Git options in disguise, and paths out of the clone root.
            "git:-utouch /tmp/x; git-upload-pack #@x.y:a/b",
            "git:github.com/user/repo@--upload-pack=x",
            "git:github.com/-x/repo",
            "git:../skills/foo",
            "https://../a/b",
            "git:github.com/./repo",
        ] {
            assert!(Source::parse(bad, Path::new("/"), None).is_err(), "{bad}");
        }
    }

    #[test]
    fn other_specs_are_local_paths() {
        let home = Path::new("/home/u");
        let p = |s: &str| Source::parse(s, Path::new("/w/proj"), Some(home)).unwrap();
        assert_eq!(p("./pkg"), Source::Local("/w/proj/pkg".into()));
        assert_eq!(p("../pkg"), Source::Local("/w/pkg".into()));
        assert_eq!(p("~/pkgs/a"), Source::Local("/home/u/pkgs/a".into()));
        // Shorthand without git: is a path (pi parity).
        assert_eq!(
            p("github.com/user/repo"),
            Source::Local("/w/proj/github.com/user/repo".into())
        );
    }

    #[test]
    fn contents_by_manifest_or_convention() {
        let d = tempfile::tempdir().unwrap();
        let conv = d.path().join("conv");
        for sub in ["extensions", "skills", "prompts"] {
            std::fs::create_dir_all(conv.join(sub)).unwrap();
        }
        let c = contents(&conv);
        assert_eq!(c.extension_dirs, [conv.join("extensions")]);
        assert_eq!((c.skills.len(), c.prompts.len(), c.themes.len()), (1, 1, 0));
        let man = d.path().join("man");
        std::fs::create_dir_all(&man).unwrap();
        std::fs::write(
            man.join("package.json"),
            r#"{"pi": {"extensions": ["x.ts"], "skills": ["s"], "prompts": ["../escape"]}}"#,
        )
        .unwrap();
        let c = contents(&man);
        assert!(
            c.extensions.is_empty(),
            "pi's TypeScript extensions aren't loaded"
        );
        assert_eq!(c.skills, [man.join("s")]);
        assert!(
            c.prompts.is_empty(),
            "paths outside the package are skipped"
        );
        let one = d.path().join("one");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::write(one.join("init.lua"), "return {}").unwrap();
        assert_eq!(contents(&one).extensions, std::slice::from_ref(&one));
    }

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A git package clones, pins to a tag, and follows its branch when
    /// unpinned.
    #[test]
    fn git_packages_clone_pin_and_update() {
        let d = tempfile::tempdir().unwrap();
        let up = d.path().join("upstream");
        std::fs::create_dir_all(up.join("skills/hello")).unwrap();
        std::fs::write(
            up.join("skills/hello/SKILL.md"),
            "---\ndescription: v1\n---\n",
        )
        .unwrap();
        sh(&up, &["init", "-q", "-b", "main"]);
        sh(&up, &["add", "."]);
        sh(&up, &["commit", "-qm", "one"]);
        sh(&up, &["tag", "v1"]);
        std::fs::write(
            up.join("skills/hello/SKILL.md"),
            "---\ndescription: v2\n---\n",
        )
        .unwrap();
        sh(&up, &["commit", "-qam", "two"]);
        let src = |r: Option<&str>| GitSource {
            repo: up.display().to_string(),
            host: "example.com".into(),
            path: "u/r".into(),
            git_ref: r.map(str::to_owned),
        };
        let read = |dir: &Path| std::fs::read_to_string(dir.join("skills/hello/SKILL.md")).unwrap();
        let pinned = d.path().join("pinned");
        fetch(&src(Some("v1")), &pinned).unwrap();
        assert!(read(&pinned).contains("v1"));
        assert_eq!(contents(&pinned).skills, [pinned.join("skills")]);
        let follow = d.path().join("follow");
        fetch(&src(None), &follow).unwrap();
        assert!(read(&follow).contains("v2"));
        std::fs::write(
            up.join("skills/hello/SKILL.md"),
            "---\ndescription: v3\n---\n",
        )
        .unwrap();
        sh(&up, &["commit", "-qam", "three"]);
        fetch(&src(None), &follow).unwrap();
        assert!(read(&follow).contains("v3"), "unpinned follows the branch");
        fetch(&src(Some("v1")), &pinned).unwrap();
        assert!(read(&pinned).contains("v1"), "pinned stays");
        // A failed clone leaves nothing behind.
        let mut bad = src(None);
        bad.repo = d.path().join("missing").display().to_string();
        let gone = d.path().join("gone");
        assert!(fetch(&bad, &gone).is_err());
        assert!(!gone.exists());
    }

    /// An untrusted project's packages aren't fetched: its settings come
    /// with the repository.
    #[test]
    fn update_skips_an_untrusted_projects_packages() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths {
            home: d.path().join("home"),
            cwd: d.path().join("project"),
        };
        std::fs::create_dir_all(paths.cwd.join(".psi")).unwrap();
        std::fs::write(
            paths.settings(Scope::Project),
            r#"{"packages": ["git:example.invalid/u/r"]}"#,
        )
        .unwrap();
        let done = update(&paths, None, false).unwrap();
        assert_eq!(done.len(), 1, "{done:?}");
        assert!(
            done[0].starts_with("skipped the project's packages"),
            "{done:?}"
        );
        assert!(!paths.git_root(Scope::Project).exists());
    }

    /// A clone that fails into a directory that was already there leaves it.
    #[test]
    fn a_failed_clone_keeps_a_directory_it_did_not_make() {
        let d = tempfile::tempdir().unwrap();
        let mine = d.path().join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(mine.join("keep.txt"), "x").unwrap();
        let bad = GitSource {
            repo: d.path().join("missing").display().to_string(),
            host: "example.com".into(),
            path: "u/r".into(),
            git_ref: None,
        };
        assert!(fetch(&bad, &mine).is_err());
        assert!(mine.join("keep.txt").exists());
    }

    #[test]
    fn settings_edits_keep_comments() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("settings.jsonc");
        std::fs::write(&f, "// mine\n{\n  \"mode\": \"auto\", // keep\n}\n").unwrap();
        assert!(
            edit_packages(&f, |l| {
                l.push("a".into());
                true
            })
            .unwrap()
        );
        assert!(
            edit_packages(&f, |l| {
                l.push("b".into());
                true
            })
            .unwrap()
        );
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(
            text.starts_with("// mine\n") && text.contains("// keep"),
            "{text}"
        );
        let v = crate::config::parse_jsonc(&text, "t").unwrap();
        assert_eq!(v["packages"], serde_json::json!(["a", "b"]));
        assert_eq!(v["mode"], "auto");
    }
}
