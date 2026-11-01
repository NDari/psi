//! Self-knowledge (psi-plan.md §10, D-28): `psi help topics` and `psi help
//! <topic>` print short answers about psi from docs/help, embedded in the
//! binary. The system prompt tells the model to run them when asked about
//! psi itself.

/// (name, text): each file's first line is its one-line summary.
pub const TOPICS: &[(&str, &str)] = &[
    ("commands", include_str!("../../../docs/help/commands.md")),
    (
        "extensions",
        include_str!("../../../docs/help/extensions.md"),
    ),
    (
        "keybindings",
        include_str!("../../../docs/help/keybindings.md"),
    ),
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
];

/// The topic list, one per line with its summary.
pub fn topics() -> String {
    let mut s = String::from("psi help <topic> for one of:\n");
    for (name, text) in TOPICS {
        s.push_str(&format!(
            "  {name:<16}{}\n",
            text.lines().next().unwrap_or("")
        ));
    }
    s
}

/// A topic's text; a close name ("plan-mode", "keys") finds it too.
pub fn topic(name: &str) -> Option<&'static str> {
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
    fn lookup() {
        assert!(topic("plan").unwrap().starts_with("Plan mode"));
        assert!(topic("plan-mode").unwrap().starts_with("Plan mode"));
        assert!(topic("keys").unwrap().starts_with("Keybindings"));
        assert!(topic("nonsense").is_none());
        assert!(topics().contains("  skills          Skills: "));
    }
}
