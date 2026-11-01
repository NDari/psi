//! The example extensions (examples/extensions), embedded in the binary so
//! `psi ext example` can show them wherever psi is installed.

pub struct Example {
    pub name: &'static str,
    /// What it shows, for the list.
    pub shows: &'static str,
    /// (file name, contents), tests included.
    pub files: &'static [(&'static str, &'static str)],
}

macro_rules! file {
    ($dir:literal, $file:literal) => {
        (
            $file,
            include_str!(concat!("../../../examples/extensions/", $dir, "/", $file)),
        )
    };
}

pub const EXAMPLES: &[Example] = &[
    Example {
        name: "ask-first",
        shows: "UI: confirm, select, input, a custom component; a tool that always asks",
        files: &[
            file!("ask-first", "init.lua"),
            file!("ask-first", "ask_first_test.lua"),
        ],
    },
    Example {
        name: "focus",
        shows: "a flag, a shortcut, the event bus, choosing the active tools",
        files: &[file!("focus", "init.lua"), file!("focus", "focus_test.lua")],
    },
    Example {
        name: "git-guard",
        shows: "a tool_call gate that refuses with a reason",
        files: &[
            file!("git-guard", "init.lua"),
            file!("git-guard", "git_guard_test.lua"),
        ],
    },
    Example {
        name: "llama-box",
        shows: "a provider (another OpenAI-compatible server)",
        files: &[
            file!("llama-box", "init.lua"),
            file!("llama-box", "llama_box_test.lua"),
        ],
    },
    Example {
        name: "tally",
        shows: "session entries that follow the branch, status line, widget, commands, messages",
        files: &[file!("tally", "init.lua"), file!("tally", "tally_test.lua")],
    },
    Example {
        name: "tidy-results",
        shows: "a tool_result rewrite, a tool renderer, a system prompt line",
        files: &[
            file!("tidy-results", "init.lua"),
            file!("tidy-results", "tidy_results_test.lua"),
        ],
    },
    Example {
        name: "word-count",
        shows: "a script tool: tool.json and any executable",
        files: &[
            file!("word-count", "tool.json"),
            file!("word-count", "run.py"),
        ],
    },
];

/// The list, one example per line.
pub fn list() -> String {
    let mut s = String::from("psi ext example <name> prints one, tests included:\n");
    for e in EXAMPLES {
        s.push_str(&format!("  {:<14}{}\n", e.name, e.shows));
    }
    s
}

/// Every file of one example, each under a `--- name/file ---` line.
pub fn show(name: &str) -> Option<String> {
    let e = EXAMPLES.iter().find(|e| e.name == name.trim())?;
    let mut s = String::new();
    for (file, text) in e.files {
        s.push_str(&format!("--- {}/{file} ---\n{text}", e.name));
        if !text.ends_with('\n') {
            s.push('\n');
        }
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every example directory and file is embedded.
    #[test]
    fn every_example_is_embedded() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/extensions");
        let mut on_disk: Vec<String> = Vec::new();
        for d in std::fs::read_dir(&dir).unwrap().flatten() {
            for f in std::fs::read_dir(d.path()).unwrap().flatten() {
                on_disk.push(format!(
                    "{}/{}",
                    d.file_name().to_string_lossy(),
                    f.file_name().to_string_lossy()
                ));
            }
        }
        on_disk.sort();
        let mut embedded: Vec<String> = EXAMPLES
            .iter()
            .flat_map(|e| e.files.iter().map(move |(f, _)| format!("{}/{f}", e.name)))
            .collect();
        embedded.sort();
        assert_eq!(
            on_disk, embedded,
            "add the example to EXAMPLES in examples.rs"
        );
    }

    #[test]
    fn list_and_show() {
        assert!(list().contains("  git-guard     a tool_call gate"));
        let s = show("tally").unwrap();
        assert!(s.starts_with("--- tally/init.lua ---\n-- tally:"), "{s}");
        assert!(s.contains("--- tally/tally_test.lua ---\n"));
        assert!(show("nope").is_none());
    }
}
