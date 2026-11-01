//! Skills and prompt templates (psi-plan.md §10, pi parity, D-35).
//!
//! Skills are `SKILL.md` files (or loose `.md` files) with a name and a
//! description; only the descriptions go in the system prompt, and
//! `/skill:name` puts a whole skill into the conversation. Prompt templates
//! are `prompts/name.md`, run as `/name args`. The first of a name wins, in
//! the order the directories are given.

use std::path::{Path, PathBuf};

/// Pi's limits for skill names and descriptions.
const MAX_NAME: usize = 64;
const MAX_DESCRIPTION: usize = 1024;
/// A template without a description shows this much of its first line.
const DESCRIPTION_FALLBACK: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// The skill's markdown file.
    pub path: PathBuf,
    /// `disable-model-invocation: true` leaves it out of the system prompt;
    /// `/skill:name` still works.
    pub model_invocable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
    pub body: String,
}

/// How a skills directory is laid out (pi parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillLayout {
    /// `~/.psi/skills`, `.psi/skills`, settings paths: `.md` files at the
    /// top, or directories holding `SKILL.md`.
    Psi,
    /// `.agents/skills`: directories holding `SKILL.md`, or `.md` files one
    /// level down; top-level `.md` files are not skills.
    Agents,
}

/// Where to look, in order of precedence (the first of a name wins).
#[derive(Debug, Clone, Default)]
pub struct Sources {
    pub skills: Vec<(PathBuf, SkillLayout)>,
    pub prompts: Vec<PathBuf>,
    pub themes: Vec<PathBuf>,
    /// Sub-agent definitions (§10.1).
    pub agents: Vec<PathBuf>,
}

impl Sources {
    /// The standard places: the project's own (when trusted) before the
    /// user's, then `extra` (settings and packages) after each.
    pub fn standard(
        home: Option<&Path>,
        cwd: &Path,
        trusted: bool,
        extra_skills: &[PathBuf],
        extra_prompts: &[PathBuf],
    ) -> Self {
        let mut s = Sources::default();
        if trusted {
            s.skills.push((cwd.join(".psi/skills"), SkillLayout::Psi));
            for d in agents_dirs(cwd, home) {
                s.skills.push((d, SkillLayout::Agents));
            }
            s.prompts.push(cwd.join(".psi/prompts"));
            s.themes.push(cwd.join(".psi/themes"));
            s.agents.push(cwd.join(".psi/agents"));
        }
        if let Some(h) = home {
            s.skills.push((h.join(".psi/skills"), SkillLayout::Psi));
            s.skills
                .push((h.join(".agents/skills"), SkillLayout::Agents));
            s.prompts.push(h.join(".psi/prompts"));
            s.themes.push(h.join(".psi/themes"));
            s.agents.push(h.join(".psi/agents"));
        }
        s.skills
            .extend(extra_skills.iter().map(|p| (p.clone(), SkillLayout::Psi)));
        s.prompts.extend(extra_prompts.iter().cloned());
        s
    }
}

/// `.agents/skills` in `cwd` and each parent up to the git root (or the
/// filesystem root outside a repository), leaving out the home one.
pub fn agents_dirs(cwd: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in cwd.ancestors() {
        if Some(d) != home {
            out.push(d.join(".agents/skills"));
        }
        if d.join(".git").exists() {
            break;
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct Resources {
    pub skills: Vec<Skill>,
    pub templates: Vec<Template>,
    /// Theme files (`themes/*.json`); the built-in default isn't listed.
    pub themes: Vec<psi_tui::theme::Theme>,
    /// Sub-agents from files; the built-in ones are added where used.
    pub agents: Vec<psi_agent::subagent::AgentDef>,
    /// Said once at startup.
    pub warnings: Vec<String>,
}

impl Resources {
    pub fn load(src: &Sources) -> Self {
        let mut r = Resources::default();
        for (dir, layout) in &src.skills {
            let mut files = Vec::new();
            if dir.is_file() {
                files.push(dir.clone());
            } else {
                skill_files(dir, *layout, true, &mut files);
            }
            for f in files {
                r.add_skill(&f);
            }
        }
        for dir in &src.prompts {
            let mut files: Vec<PathBuf> = if dir.is_file() {
                vec![dir.clone()]
            } else {
                visible(dir)
                    .into_iter()
                    .filter(|p| p.is_file() && is_md(p))
                    .collect()
            };
            files.sort();
            for f in files {
                r.add_template(&f);
            }
        }
        for dir in &src.agents {
            let mut files: Vec<PathBuf> = visible(dir)
                .into_iter()
                .filter(|p| p.is_file() && is_md(p))
                .collect();
            files.sort();
            for f in files {
                r.add_agent(&f);
            }
        }
        for dir in &src.themes {
            let files: Vec<PathBuf> = if dir.is_file() {
                vec![dir.clone()]
            } else {
                visible(dir)
                    .into_iter()
                    .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "json"))
                    .collect()
            };
            for f in files {
                r.add_theme(&f);
            }
        }
        r
    }

    fn add_theme(&mut self, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return self.warn(path, "cannot be read"),
        };
        let v = match crate::config::parse_jsonc(&text, &path.display().to_string()) {
            Ok(v) => v,
            Err(e) => return self.warnings.push(format!("{e:#}")),
        };
        match psi_tui::theme::Theme::from_json(&v) {
            Ok((t, problems)) => {
                for p in problems {
                    self.warn(path, &p);
                }
                if t.name == "default" || self.themes.iter().any(|x| x.name == t.name) {
                    return self.warn(
                        path,
                        &format!("theme {} is defined earlier, which wins", t.name),
                    );
                }
                self.themes.push(t);
            }
            Err(e) => self.warn(path, &e),
        }
    }

    /// A theme by name; "default" is the built-in one.
    pub fn theme(&self, name: &str) -> Option<psi_tui::theme::Theme> {
        if name == "default" {
            return Some(psi_tui::theme::Theme::default());
        }
        self.themes.iter().find(|t| t.name == name).cloned()
    }

    fn add_skill(&mut self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return self.warn(path, "cannot be read");
        };
        let is_skill_md = path.file_name().is_some_and(|n| n == "SKILL.md");
        let (fm, _) = match frontmatter(&text) {
            Ok(x) => x,
            // A loose .md that isn't a skill is skipped quietly (pi parity).
            Err(e) if is_skill_md => return self.warn(path, &e),
            Err(_) => return,
        };
        let description = fm.get("description").unwrap_or("").trim().to_owned();
        if description.is_empty() {
            if is_skill_md {
                self.warn(path, "has no description, so it isn't loaded");
            }
            return;
        }
        let name = match fm.get("name").filter(|n| !n.trim().is_empty()) {
            Some(n) => n.trim().to_owned(),
            None if is_skill_md => dir_name(path),
            None => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        if let Some(problem) = name_problem(&name) {
            self.warn(path, &format!("skill name '{name}' {problem}"));
        }
        if description.chars().count() > MAX_DESCRIPTION {
            self.warn(
                path,
                &format!("description is longer than {MAX_DESCRIPTION} characters"),
            );
        }
        if let Some(first) = self.skills.iter().find(|s| s.name == name) {
            let msg = format!(
                "skill '{name}' is also in {}, which wins",
                first.path.display()
            );
            return self.warn(path, &msg);
        }
        self.skills.push(Skill {
            name,
            description,
            path: path.to_owned(),
            model_invocable: fm.get("disable-model-invocation") != Some("true"),
        });
    }

    fn add_template(&mut self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return self.warn(path, "cannot be read");
        };
        let (fm, body) = match frontmatter(&text) {
            Ok(x) => x,
            Err(e) => return self.warn(path, &e),
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.templates.iter().any(|t| t.name == name) {
            return self.warn(
                path,
                &format!("prompt /{name} is defined earlier, which wins"),
            );
        }
        let description = match fm.get("description").filter(|d| !d.trim().is_empty()) {
            Some(d) => d.trim().to_owned(),
            None => {
                let first = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                if first.chars().count() > DESCRIPTION_FALLBACK {
                    let cut: String = first.chars().take(DESCRIPTION_FALLBACK).collect();
                    format!("{cut}...")
                } else {
                    first.to_owned()
                }
            }
        };
        self.templates.push(Template {
            name,
            description,
            argument_hint: fm.get("argument-hint").map(str::to_owned),
            body,
        });
    }

    /// A sub-agent file (pi's format): `name`, `description`, `tools` (a
    /// comma list) frontmatter; the body is its prompt.
    fn add_agent(&mut self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return self.warn(path, "cannot be read");
        };
        let (fm, body) = match frontmatter(&text) {
            Ok(x) => x,
            Err(e) => return self.warn(path, &e),
        };
        let description = fm.get("description").unwrap_or("").trim().to_owned();
        if description.is_empty() {
            return self.warn(path, "has no description, so the agent isn't loaded");
        }
        let name = match fm.get("name").filter(|n| !n.trim().is_empty()) {
            Some(n) => n.trim().to_owned(),
            None => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        if let Some(problem) = name_problem(&name) {
            return self.warn(path, &format!("agent name '{name}' {problem}"));
        }
        if self.agents.iter().any(|a| a.name == name) {
            return self.warn(
                path,
                &format!("agent '{name}' is defined earlier, which wins"),
            );
        }
        if fm.get("model").is_some() {
            self.warn(
                path,
                "model isn't supported yet; the agent uses the session's model",
            );
        }
        let tools = fm.get("tools").map(|t| {
            t.split(',')
                .map(|n| n.trim().to_lowercase())
                .filter(|n| !n.is_empty())
                .collect()
        });
        self.agents.push(psi_agent::subagent::AgentDef {
            name,
            description,
            tools,
            prompt: body.trim().to_owned(),
        });
    }

    fn warn(&mut self, path: &Path, what: &str) {
        self.warnings.push(format!("{}: {what}", path.display()));
    }

    pub fn skill(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// What the system prompt lists.
    pub fn prompt_skills(&self) -> Vec<psi_agent::prompt::SkillEntry> {
        self.skills
            .iter()
            .filter(|s| s.model_invocable)
            .map(|s| psi_agent::prompt::SkillEntry {
                name: s.name.clone(),
                description: s.description.clone(),
                path: s.path.clone(),
            })
            .collect()
    }

    /// `/skill:name args` and `/template args` become the text the model
    /// gets. `Ok(None)` means the input isn't one of them.
    pub fn expand(&self, input: &str) -> Result<Option<Expanded>, String> {
        let Some(rest) = input.strip_prefix('/') else {
            return Ok(None);
        };
        if let Some(call) = rest.strip_prefix("skill:") {
            let (name, args) = call
                .split_once(char::is_whitespace)
                .map(|(n, a)| (n, a.trim()))
                .unwrap_or((call, ""));
            let Some(s) = self.skill(name) else {
                return Err(format!("no skill named '{name}'"));
            };
            let text = std::fs::read_to_string(&s.path)
                .map_err(|e| format!("cannot read {}: {e}", s.path.display()))?;
            let body = frontmatter(&text).map(|(_, b)| b).unwrap_or(text);
            let dir = s.path.parent().unwrap_or(Path::new("/"));
            let mut out = format!(
                "<skill name=\"{}\" location=\"{}\">\nRelative paths in this skill are relative to {}.\n\n{}\n</skill>",
                s.name,
                s.path.display(),
                dir.display(),
                body.trim()
            );
            if !args.is_empty() {
                out.push_str("\n\n");
                out.push_str(args);
            }
            return Ok(Some(Expanded {
                text: out,
                note: format!("skill {}", s.name),
            }));
        }
        let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let Some(t) = self.templates.iter().find(|t| t.name == name) else {
            return Ok(None);
        };
        Ok(Some(Expanded {
            text: substitute(&t.body, &split_args(args)),
            note: format!("prompt /{}", t.name),
        }))
    }

    /// Completion entries: (`/name`, description).
    pub fn commands(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self
            .templates
            .iter()
            .map(|t| {
                let d = match &t.argument_hint {
                    Some(h) if t.description.is_empty() => h.clone(),
                    Some(h) => format!("{h}: {}", t.description),
                    None => t.description.clone(),
                };
                (format!("/{}", t.name), d)
            })
            .collect();
        v.extend(
            self.skills
                .iter()
                .map(|s| (format!("/skill:{}", s.name), first_line(&s.description))),
        );
        v
    }
}

/// An expanded command: the text sent, and a short note for the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    pub text: String,
    pub note: String,
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_owned()
}

fn is_md(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "md")
}

/// A directory's entries, without hidden ones and `node_modules`.
fn visible(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| !n.to_string_lossy().starts_with('.') && n != "node_modules")
        })
        .collect();
    v.sort();
    v
}

fn skill_files(dir: &Path, layout: SkillLayout, top: bool, out: &mut Vec<PathBuf>) {
    let skill_md = dir.join("SKILL.md");
    if skill_md.is_file() {
        out.push(skill_md);
        return;
    }
    for p in visible(dir) {
        if p.is_dir() {
            skill_files(&p, layout, false, out);
        } else if is_md(&p) {
            let loose = match layout {
                SkillLayout::Psi => top,
                SkillLayout::Agents => !top,
            };
            if loose {
                out.push(p);
            }
        }
    }
}

fn dir_name(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Pi's name rules; breaking one only warns.
fn name_problem(name: &str) -> Option<&'static str> {
    if name.chars().count() > MAX_NAME {
        Some("is longer than 64 characters")
    } else if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        Some("should use only a-z, 0-9 and hyphens")
    } else if name.starts_with('-') || name.ends_with('-') {
        Some("should not start or end with a hyphen")
    } else if name.contains("--") {
        Some("should not contain two hyphens in a row")
    } else {
        None
    }
}

/// The top-level `key: value` pairs of a frontmatter block.
#[derive(Debug, Default)]
pub struct Frontmatter(Vec<(String, String)>);

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Splits `---` frontmatter from the body. Only the YAML that skills and
/// templates use is read: top-level scalars, quoted or plain, and `|` or
/// `>` blocks. Nested keys are skipped.
pub fn frontmatter(text: &str) -> Result<(Frontmatter, String), String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let Some(rest) = text.strip_prefix("---\n") else {
        return Ok((Frontmatter::default(), text));
    };
    let (yaml, body) = match rest.find("\n---") {
        Some(i) => {
            let after = &rest[i + 4..];
            let body = after.split_once('\n').map(|(_, b)| b).unwrap_or("");
            (&rest[..i], body.trim().to_owned())
        }
        None if rest.starts_with("---") => ("", rest[3..].trim().to_owned()),
        None => return Ok((Frontmatter::default(), text)),
    };
    let mut out = Vec::new();
    let lines: Vec<&str> = yaml.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') || line.starts_with(' ') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            return Err(format!("frontmatter line {i} is not 'key: value'"));
        };
        let v = v.trim();
        let value = if v.starts_with('|') || v.starts_with('>') {
            let mut block = Vec::new();
            while i < lines.len() && (lines[i].starts_with(' ') || lines[i].trim().is_empty()) {
                block.push(lines[i].trim());
                i += 1;
            }
            if v.starts_with('>') {
                block.join(" ").trim().to_owned()
            } else {
                block.join("\n").trim_end().to_owned()
            }
        } else if let Some(q) = v.strip_prefix('"') {
            let Some(inner) = q.strip_suffix('"') else {
                return Err(format!("frontmatter line {i}: unclosed quote"));
            };
            inner.replace("\\\"", "\"").replace("\\n", "\n")
        } else if let Some(q) = v.strip_prefix('\'') {
            let Some(inner) = q.strip_suffix('\'') else {
                return Err(format!("frontmatter line {i}: unclosed quote"));
            };
            inner.replace("''", "'")
        } else {
            v.to_owned()
        };
        out.push((k.trim().to_owned(), value));
    }
    Ok((Frontmatter(out), body))
}

/// Shell-like splitting for template arguments (pi parity): whitespace
/// separates, quotes group and are removed, no backslash escapes.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            None => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// Fills a template (pi parity): `$1`, `$@` and `$ARGUMENTS`, `${N:-default}`,
/// `${@:-default}`, `${@:N}` and `${@:N:L}`. One pass; argument text is
/// never substituted again.
pub fn substitute(body: &str, args: &[String]) -> String {
    let all = args.join(" ");
    let arg = |n: usize| -> String {
        n.checked_sub(1)
            .and_then(|i| args.get(i))
            .cloned()
            .unwrap_or_default()
    };
    let slice = |start: usize, len: Option<usize>| -> String {
        let from = start.max(1) - 1;
        if from >= args.len() {
            return String::new();
        }
        let to = len.map_or(args.len(), |l| (from + l).min(args.len()));
        args[from..to].join(" ")
    };
    let mut out = String::new();
    let b = body.as_bytes();
    let mut i = 0;
    while i < body.len() {
        if b[i] != b'$' {
            let ch = body[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let rest = &body[i + 1..];
        if let Some(inner) = rest.strip_prefix('{')
            && let Some(end) = inner.find('}')
        {
            let expr = &inner[..end];
            let done = if let Some((what, def)) = expr.split_once(":-") {
                let v = match what {
                    "@" | "ARGUMENTS" => Some(all.clone()),
                    n if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => {
                        n.parse().ok().map(arg)
                    }
                    _ => None,
                };
                v.map(|v| if v.is_empty() { def.to_owned() } else { v })
            } else if let Some(range) = expr.strip_prefix("@:") {
                let mut parts = range.splitn(2, ':');
                let start = parts.next().and_then(|s| s.parse::<usize>().ok());
                let len = parts.next().map(|s| s.parse::<usize>().ok());
                match (start, len) {
                    (Some(s), None) => Some(slice(s, None)),
                    (Some(s), Some(Some(l))) => Some(slice(s, Some(l))),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(v) = done {
                out.push_str(&v);
                i += 1 + 1 + end + 1;
                continue;
            }
        }
        if rest.starts_with("ARGUMENTS") {
            out.push_str(&all);
            i += 1 + "ARGUMENTS".len();
            continue;
        }
        if rest.starts_with('@') {
            out.push_str(&all);
            i += 2;
            continue;
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            let n: usize = rest[..digits].parse().unwrap_or(0);
            out.push_str(&arg(n));
            i += 1 + digits;
            continue;
        }
        out.push('$');
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn frontmatter_forms() {
        let (fm, body) = frontmatter(
            "---\nname: pdf\ndescription: >\n  Reads PDFs\n  and tables.\nquoted: \"a: b\"\nnested:\n  x: 1\n---\n\n# Body\n",
        )
        .unwrap();
        assert_eq!(fm.get("name"), Some("pdf"));
        assert_eq!(fm.get("description"), Some("Reads PDFs and tables."));
        assert_eq!(fm.get("quoted"), Some("a: b"));
        assert_eq!(fm.get("x"), None);
        assert_eq!(body, "# Body");
        let (fm, body) = frontmatter("no frontmatter\n").unwrap();
        assert_eq!(fm.get("name"), None);
        assert_eq!(body, "no frontmatter\n");
        assert!(frontmatter("---\nnot yaml\n---\nx").is_err());
    }

    #[test]
    fn skills_layouts_names_and_precedence() {
        let d = tempfile::tempdir().unwrap();
        let user = d.path().join("user");
        let proj = d.path().join("proj");
        write(
            &user.join("pdf/SKILL.md"),
            "---\ndescription: user pdf\n---\nuser body",
        );
        write(
            &user.join("pdf/nested/SKILL.md"),
            "---\nname: hidden\ndescription: not found below a SKILL.md\n---\n",
        );
        write(
            &user.join("loose.md"),
            "---\ndescription: a loose skill\n---\n",
        );
        write(&user.join("notes.md"), "just notes, no frontmatter");
        write(&user.join("bad/SKILL.md"), "no description here");
        write(
            &user.join("Odd_Name/SKILL.md"),
            "---\ndescription: odd\ndisable-model-invocation: true\n---\n",
        );
        write(
            &proj.join("pdf/SKILL.md"),
            "---\ndescription: project pdf\n---\n",
        );
        write(
            &proj.join("top.md"),
            "---\ndescription: not a skill here\n---\n",
        );
        write(
            &proj.join("group/deploy.md"),
            "---\ndescription: deploys\n---\n",
        );
        let src = Sources {
            skills: vec![
                (proj.clone(), SkillLayout::Agents),
                (user.clone(), SkillLayout::Psi),
            ],
            prompts: vec![],
            themes: vec![],
            agents: vec![],
        };
        let r = Resources::load(&src);
        let names: Vec<(&str, &str)> = r
            .skills
            .iter()
            .map(|s| (s.name.as_str(), s.description.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("deploy", "deploys"),
                ("pdf", "project pdf"),
                ("Odd_Name", "odd"),
                ("loose", "a loose skill"),
            ]
        );
        let w = r.warnings.join("\n");
        assert!(w.contains("bad/SKILL.md: has no description"), "{w}");
        assert!(w.contains("skill name 'Odd_Name' should use only"), "{w}");
        assert!(w.contains("skill 'pdf' is also in"), "{w}");
        assert!(!w.contains("notes.md"), "{w}");
        // disable-model-invocation keeps it out of the prompt only.
        let listed: Vec<String> = r.prompt_skills().into_iter().map(|s| s.name).collect();
        assert_eq!(listed, ["deploy", "pdf", "loose"]);
        assert!(r.expand("/skill:Odd_Name").unwrap().is_some());
    }

    #[test]
    fn agents_load_in_pi_format_and_the_project_wins() {
        let d = tempfile::tempdir().unwrap();
        let (proj, user) = (d.path().join("p"), d.path().join("u"));
        write(
            &proj.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Reviews a change\ntools: Read, bash\nmodel: big\n---\n\nYou are reviewer.\n",
        );
        write(
            &user.join("reviewer.md"),
            "---\ndescription: the user's\n---\nx\n",
        );
        write(
            &user.join("helper.md"),
            "---\ndescription: Helps\n---\nHelp.\n",
        );
        write(
            &user.join("bad.md"),
            "---\nname: Bad\ndescription: x\n---\n",
        );
        write(&user.join("none.md"), "no frontmatter\n");
        let r = Resources::load(&Sources {
            agents: vec![proj, user],
            ..Sources::default()
        });
        let got: Vec<(&str, &str, Option<Vec<String>>, &str)> = r
            .agents
            .iter()
            .map(|a| {
                (
                    a.name.as_str(),
                    a.description.as_str(),
                    a.tools.clone(),
                    a.prompt.as_str(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (
                    "reviewer",
                    "Reviews a change",
                    Some(vec!["read".into(), "bash".into()]),
                    "You are reviewer."
                ),
                ("helper", "Helps", None, "Help."),
            ]
        );
        let w = r.warnings.join("\n");
        assert!(w.contains("model isn't supported yet"), "{w}");
        assert!(w.contains("agent 'reviewer' is defined earlier"), "{w}");
        assert!(w.contains("agent name 'Bad' should use only"), "{w}");
        assert!(w.contains("none.md: has no description"), "{w}");
    }

    #[test]
    fn skill_command_inserts_the_body() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("s/pdf/SKILL.md");
        write(&f, "---\ndescription: pdf\n---\n\nUse scripts/x.py.\n");
        let r = Resources::load(&Sources {
            skills: vec![(d.path().join("s"), SkillLayout::Psi)],
            prompts: vec![],
            themes: vec![],
            agents: vec![],
        });
        let e = r.expand("/skill:pdf  read report.pdf").unwrap().unwrap();
        let dir = f.parent().unwrap().display().to_string();
        assert_eq!(
            e.text,
            format!(
                "<skill name=\"pdf\" location=\"{}\">\nRelative paths in this skill are relative to {dir}.\n\nUse scripts/x.py.\n</skill>\n\nread report.pdf",
                f.display()
            )
        );
        assert_eq!(e.note, "skill pdf");
        assert!(r.expand("/skill:nope").is_err());
        assert_eq!(r.expand("plain text").unwrap(), None);
    }

    #[test]
    fn templates_and_descriptions() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("prompts");
        write(
            &p.join("review.md"),
            "---\ndescription: Review a file\nargument-hint: <file>\n---\nReview $1 for $ARGUMENTS.",
        );
        write(
            &p.join("plain.md"),
            "\n# A heading that is the description\nBody",
        );
        write(
            &p.join("sub/deep.md"),
            "not loaded: prompts aren't recursive",
        );
        let r = Resources::load(&Sources {
            skills: vec![],
            prompts: vec![p.clone()],
            themes: vec![],
            agents: vec![],
        });
        assert_eq!(r.templates.len(), 2);
        assert_eq!(
            r.commands(),
            [
                (
                    "/plain".to_owned(),
                    "# A heading that is the description".to_owned()
                ),
                ("/review".to_owned(), "<file>: Review a file".to_owned()),
            ]
        );
        let e = r
            .expand("/review a.rs \"bugs and style\"")
            .unwrap()
            .unwrap();
        assert_eq!(e.text, "Review a.rs for a.rs bugs and style.");
        assert_eq!(e.note, "prompt /review");
        assert_eq!(r.expand("/missing x").unwrap(), None);
    }

    #[test]
    fn themes_load_and_the_first_wins() {
        let d = tempfile::tempdir().unwrap();
        write(
            &d.path().join("a/one.json"),
            r##"{"name": "one", "colors": {"accent": "#ff0000"}}"##,
        );
        write(
            &d.path().join("b/again.json"),
            r#"{"name": "one", "colors": {}}"#,
        );
        write(&d.path().join("b/bad.json"), "{");
        let r = Resources::load(&Sources {
            themes: vec![d.path().join("a"), d.path().join("b")],
            ..Default::default()
        });
        assert_eq!(r.themes.len(), 1);
        assert!(r.theme("one").is_some() && r.theme("default").is_some());
        assert!(r.theme("two").is_none());
        assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
    }

    #[test]
    fn argument_splitting() {
        assert_eq!(split_args("a  'b c' \"d\"e"), ["a", "b c", "de"]);
        assert_eq!(split_args("a '' c"), ["a", "", "c"]);
        assert_eq!(split_args("\"open quote runs on"), ["open quote runs on"]);
        assert!(split_args("  ").is_empty());
    }

    #[test]
    fn substitution() {
        let a: Vec<String> = ["x", "y", "z"].iter().map(|s| s.to_string()).collect();
        assert_eq!(substitute("$1-$2-$3-$4", &a), "x-y-z-");
        assert_eq!(substitute("[$@] [$ARGUMENTS]", &a), "[x y z] [x y z]");
        assert_eq!(substitute("${2:-def} ${5:-def}", &a), "y def");
        assert_eq!(substitute("${@:2} | ${@:1:2} | ${@:9}", &a), "y z | x y | ");
        assert_eq!(substitute("${@:-none}", &[]), "none");
        assert_eq!(
            substitute("$ $$ $A ${1} pre$@", &a),
            "$ $$ $A ${1} prex y z"
        );
        // Argument text isn't substituted again.
        let b = vec!["$2".to_owned(), "two".to_owned()];
        assert_eq!(substitute("$1", &b), "$2");
        assert_eq!(substitute("price ${1:-free} ünï", &[]), "price free ünï");
    }
}
