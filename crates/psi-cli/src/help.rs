//! Self-knowledge (psi-plan.md §10, D-28): `psi help topics` and `psi help
//! <topic>` print short answers about psi from docs/help, embedded in the
//! binary. The system prompt tells the model to run them when asked about
//! psi itself.

/// (name, text): each file's first line is its one-line summary.
pub const TOPICS: &[(&str, &str)] = &[
    ("agents", include_str!("../../../docs/help/agents.md")),
    ("commands", include_str!("../../../docs/help/commands.md")),
    (
        "extensions",
        include_str!("../../../docs/help/extensions.md"),
    ),
    (
        "keybindings",
        include_str!("../../../docs/help/keybindings.md"),
    ),
    ("mcp", include_str!("../../../docs/help/mcp.md")),
    ("models", include_str!("../../../docs/help/models.md")),
    ("packages", include_str!("../../../docs/help/packages.md")),
    ("plan", include_str!("../../../docs/help/plan.md")),
    (
        "post-edit-check",
        include_str!("../../../docs/help/post-edit-check.md"),
    ),
    ("prompts", include_str!("../../../docs/help/prompts.md")),
    ("safety", include_str!("../../../docs/help/safety.md")),
    ("sessions", include_str!("../../../docs/help/sessions.md")),
    ("settings", include_str!("../../../docs/help/settings.md")),
    ("skills", include_str!("../../../docs/help/skills.md")),
    ("themes", include_str!("../../../docs/help/themes.md")),
    ("todo", include_str!("../../../docs/help/todo.md")),
];

/// Other words for a topic.
const ALIASES: &[(&str, &str)] = &[
    ("keys", "keybindings"),
    ("shortcuts", "keybindings"),
    ("modes", "safety"),
    ("mode", "safety"),
    ("undo", "safety"),
    ("sandbox", "safety"),
    ("templates", "prompts"),
    ("check", "post-edit-check"),
    ("config", "settings"),
    ("slash", "commands"),
    ("subagents", "agents"),
];

/// `psi help api`'s line in the topic list.
pub const API_SUMMARY: &str =
    "The extension API reference: every function, event and type, one at a time.";

/// The topic list, one per line with its summary.
pub fn topics() -> String {
    let mut s = String::from("psi help <topic> for one of:\n");
    s.push_str(&format!("  {:<16}{API_SUMMARY}\n", "api"));
    for (name, text) in TOPICS {
        s.push_str(&format!(
            "  {name:<16}{}\n",
            text.lines().next().unwrap_or("")
        ));
    }
    s
}

/// The extension API reference (docs/api.md), in the binary so the model
/// can read it from any project. In the cog runs it could not: the help
/// named "docs/api.md in the psi repository", and runs spent up to 60 calls
/// in psi's Rust source for psi.exec's options, which only the reference
/// lists. 30 KB whole, so it is read one entry at a time.
const API: &str = psi_ext_api::API_MARKDOWN;

/// One heading of the reference and the text under it, up to the next
/// heading of the same or a higher level.
struct Section {
    level: usize,
    title: &'static str,
    /// The `##` part it is in: Functions, Events or Types.
    part: &'static str,
    text: &'static str,
}

fn sections() -> Vec<Section> {
    let mut heads = Vec::new();
    let (mut pos, mut code, mut part) = (0, false, "");
    for line in API.split_inclusive('\n') {
        if line.starts_with("```") {
            code = !code;
        }
        let level = line.bytes().take_while(|b| *b == b'#').count();
        if !code && (2..=4).contains(&level) && line.as_bytes().get(level) == Some(&b' ') {
            let title = line[level + 1..].trim();
            if level == 2 {
                part = title;
            }
            heads.push((pos, level, title, part));
        }
        pos += line.len();
    }
    heads
        .iter()
        .enumerate()
        .map(|(i, &(start, level, title, part))| {
            let end = heads[i + 1..]
                .iter()
                .find(|h| h.1 <= level)
                .map_or(API.len(), |h| h.0);
            Section {
                level,
                title,
                part,
                text: API[start..end].trim_end(),
            }
        })
        .collect()
}

/// Whether `name` is a whole word in `text`, ignoring case.
fn has_word(text: &str, name: &str) -> bool {
    let (text, name) = (text.to_ascii_lowercase(), name.to_ascii_lowercase());
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    text.match_indices(&name).any(|(i, _)| {
        !word(text[..i].chars().next_back()) && !word(text[i + name.len()..].chars().next())
    })
}

/// Whether the code in `text` (inside backticks) names the type `name`.
fn names_type(text: &str, name: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    text.split('`').skip(1).step_by(2).any(|code| {
        code.match_indices(name).any(|(i, _)| {
            !word(code[..i].chars().next_back()) && !word(code[i + name.len()..].chars().next())
        })
    })
}

/// `psi help api`: every function's signature, the events and the types.
pub fn api_index() -> String {
    let secs = sections();
    let mut s = String::from(
        "The extension API. psi help api <name> prints one entry with the types it uses \
         (psi help api psi.exec, psi help api ToolSpec, psi help api events); \
         psi help api all prints all of it.\n\nFunctions:\n",
    );
    for sec in secs.iter().filter(|s| s.level == 4) {
        let sig = sec
            .text
            .lines()
            .skip_while(|l| !l.starts_with("```"))
            .nth(1);
        s.push_str(&format!("  {}\n", sig.unwrap_or(sec.title)));
    }
    let events: Vec<&str> = secs
        .iter()
        .filter(|s| s.level == 2 && s.title == "Events")
        .flat_map(|s| s.text.lines())
        .filter_map(|l| l.strip_prefix("| `")?.split('`').next())
        .collect();
    s.push_str(&format!("\nEvents, for psi.on: {}\n", events.join(", ")));
    let types: Vec<&str> = secs
        .iter()
        .filter(|s| s.level == 3 && s.part == "Types")
        .map(|s| s.title)
        .collect();
    s.push_str(&format!("\nTypes: {}\n", types.join(", ")));
    s
}

/// `psi help api <name>`: one function (`psi.exec` or `exec`), group
/// (`psi.fs`), type or part, followed by every type it uses, so one call
/// answers it. Err names where an unknown name is mentioned instead.
pub fn api_entry(name: &str) -> Result<String, String> {
    let want = name.trim();
    if want.eq_ignore_ascii_case("all") {
        return Ok(API.to_owned());
    }
    let secs = sections();
    let find = |t: &str| secs.iter().find(|s| s.title.eq_ignore_ascii_case(t));
    let Some(sec) = find(want).or_else(|| find(&format!("psi.{want}"))) else {
        let mentioned: Vec<&str> = secs
            .iter()
            .filter(|s| s.level == 4 || s.part == "Types" && s.level == 3 || s.title == "Events")
            .filter(|s| has_word(s.text, want))
            .map(|s| s.title)
            .collect();
        let hint = "psi help api lists every function, event and type.";
        return Err(if mentioned.is_empty() {
            format!("No entry '{want}' in the extension API. {hint}\n")
        } else {
            format!(
                "No entry '{want}' in the extension API; it is mentioned in: {}. {hint}\n",
                mentioned.join(", ")
            )
        });
    };
    let mut out = format!("{}\n", sec.text);
    if sec.level == 2 {
        return Ok(out);
    }
    // The types it uses, and theirs, in the reference's order.
    let types: Vec<&Section> = secs
        .iter()
        .filter(|s| s.level == 3 && s.part == "Types")
        .collect();
    let mut used = vec![false; types.len()];
    let mut todo = vec![sec.text];
    while let Some(text) = todo.pop() {
        for (i, ty) in types.iter().enumerate() {
            if !used[i] && ty.title != sec.title && names_type(text, ty.title) {
                used[i] = true;
                todo.push(ty.text);
            }
        }
    }
    for (ty, _) in types.iter().zip(&used).filter(|(_, u)| **u) {
        out.push_str(&format!("\n{}\n", ty.text));
    }
    Ok(out)
}

/// A topic's text; a close name ("plan-mode", "keys") finds it too. The
/// extensions topic carries the API index and the types a tool needs: in
/// the cog runs of 2026-11-08 the model read this topic in full every time
/// but never ran `psi help api`, and went looking in psi's source instead.
pub fn topic(name: &str) -> Option<String> {
    let text = topic_file(name)?;
    if !std::ptr::eq(text, TOPICS.iter().find(|(n, _)| *n == "extensions")?.1) {
        return Some(text.to_owned());
    }
    let index = api_index();
    // Without the index's own first line, which the topic's text says.
    let index = index
        .split_once("\n\n")
        .map_or(index.as_str(), |(_, rest)| rest);
    let mut s = format!("{text}\n{index}");
    for t in EXTENSION_TYPES {
        if let Some(sec) = sections().iter().find(|s| s.title == *t) {
            s.push_str(&format!("\n{}\n", sec.text));
        }
    }
    Some(s)
}

/// The types `psi help extensions` shows whole: a tool, what it returns,
/// and running a program.
const EXTENSION_TYPES: &[&str] = &[
    "ToolSpec",
    "ParamSpec",
    "ToolResult",
    "Context",
    "ExecOptions",
    "ExecResult",
];

fn topic_file(name: &str) -> Option<&'static str> {
    let want = name.trim().to_ascii_lowercase();
    let want = want.trim_end_matches(".md");
    let want = ALIASES
        .iter()
        .find(|(a, _)| *a == want)
        .map_or(want, |(_, t)| *t);
    TOPICS
        .iter()
        .find(|(n, _)| *n == want)
        .or_else(|| {
            TOPICS
                .iter()
                .find(|(n, _)| want.starts_with(n) || n.starts_with(want))
        })
        .map(|(_, t)| *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file in docs/help is a topic, and every topic is short.
    #[test]
    fn every_help_file_is_embedded() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/help");
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_string_lossy()
                    .strip_suffix(".md")
                    .map(str::to_owned)
            })
            .collect();
        files.sort();
        let names: Vec<&str> = TOPICS.iter().map(|(n, _)| *n).collect();
        assert_eq!(files, names, "add the file to TOPICS in help.rs");
        for (n, t) in TOPICS {
            assert!(t.lines().count() <= 30, "{n} is longer than 30 lines");
        }
    }

    #[test]
    fn api_entries_come_with_the_types_they_use() {
        let index = api_index();
        assert!(
            index.contains(
                "\n  psi.exec(argv: string[], opts: ExecOptions?): ExecResult -- async\n"
            )
        );
        assert!(index.contains("tool_call, "), "{index}");
        assert!(index.contains("Types: Context, ToolSpec, "), "{index}");
        // A function, by its full or short name, with its types inlined.
        let exec = api_entry("exec").unwrap();
        assert_eq!(exec, api_entry("psi.exec").unwrap());
        assert!(exec.starts_with("#### psi.exec\n"), "{exec}");
        assert!(
            exec.contains("### ExecOptions") && exec.contains("`stdin`"),
            "{exec}"
        );
        assert!(exec.contains("### ExecResult"), "{exec}");
        assert!(!exec.contains("### ToolSpec"), "{exec}");
        // Types of types: a tool's spec, its params and what execute returns.
        let tool = api_entry("register_tool").unwrap();
        for t in [
            "ToolSpec",
            "ParamSpec",
            "ToolAnnotations",
            "Context",
            "ToolResult",
        ] {
            assert!(tool.contains(&format!("### {t}\n")), "{t}: {tool}");
        }
        // A group holds its functions; a part stands alone.
        assert!(api_entry("psi.fs").unwrap().contains("#### psi.fs.glob"));
        let events = api_entry("events").unwrap();
        assert!(events.contains("| `tool_result` |") && !events.contains("### "));
        // Every entry stays far smaller than the whole reference.
        for s in sections()
            .iter()
            .filter(|s| s.level > 2 && s.title != "psi")
        {
            let n = api_entry(s.title).unwrap().len();
            assert!(n < 8_000, "{} is {n} bytes", s.title);
        }
        // An unknown name says where it appears.
        let e = api_entry("stdin").unwrap_err();
        assert!(e.contains("it is mentioned in: ExecOptions."), "{e}");
        assert!(api_entry("zzz").unwrap_err().starts_with("No entry 'zzz'"));
        assert_eq!(api_entry("all").unwrap(), API);
    }

    #[test]
    fn the_extensions_topic_carries_the_api() {
        let t = topic("extensions").unwrap();
        assert!(t.starts_with("Extensions: "), "{t}");
        assert!(t.contains("\n  psi.register_tool(spec: ToolSpec)\n"), "{t}");
        assert!(
            t.contains("### ToolSpec\n") && t.contains("### ExecOptions\n"),
            "{t}"
        );
        assert!(t.contains("| `stdin` |"), "{t}");
        assert!(t.len() < 12_000, "{} bytes", t.len());
        // Other topics are their file alone.
        assert!(!topic("plan").unwrap().contains("Functions:"));
    }

    #[test]
    fn lookup() {
        assert!(topic("plan").unwrap().starts_with("Plan mode"));
        assert!(topic("plan-mode").unwrap().starts_with("Plan mode"));
        assert!(topic("keys").unwrap().starts_with("Keybindings"));
        assert!(topic("nonsense").is_none());
        assert!(topics().contains("  skills          Skills: "));
        assert!(topics().contains("  api             The extension API reference"));
    }
}
