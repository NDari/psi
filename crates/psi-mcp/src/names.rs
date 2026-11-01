//! The names the model sees: `mcp__<server>__<tool>` (pi, Claude Code and
//! Codex all use it), with characters other than letters, digits and `_`
//! turned into `_`, at most 64 characters (the OpenAI limit, which some
//! chat templates assume), and a hash suffix on a clash or an overflow.

use std::collections::HashSet;

pub const MAX_NAME: usize = 64;

pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// FNV-1a: stable across builds, so names in saved sessions keep meaning.
fn hash(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The exposed name of each `(server, tool)`, in order, all distinct and
/// none equal to a name in `taken` (the core and extension tools).
pub fn assign(pairs: &[(&str, &str)], taken: &[&str]) -> Vec<String> {
    let mut used: HashSet<String> = taken.iter().map(|s| (*s).to_owned()).collect();
    pairs
        .iter()
        .map(|(server, tool)| {
            let base = format!("mcp__{}__{}", sanitize(server), sanitize(tool));
            let name = if base.len() <= MAX_NAME && !used.contains(&base) {
                base
            } else {
                let suffix = format!("_{:08x}", hash(&format!("{server}\0{tool}")));
                let keep = MAX_NAME - suffix.len();
                format!("{}{suffix}", &base[..base.len().min(keep)])
            };
            used.insert(name.clone());
            name
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_prefixed_sanitized_and_distinct() {
        let long = "x".repeat(80);
        let got = assign(
            &[
                ("git-hub", "get.issue"),
                ("git_hub", "get_issue"),
                ("s", &long),
            ],
            &[],
        );
        assert_eq!(got[0], "mcp__git_hub__get_issue");
        // The same after sanitizing: a hash suffix tells them apart.
        assert!(
            got[1].starts_with("mcp__git_hub__get_issue_") && got[1].len() == 32,
            "{}",
            got[1]
        );
        assert_eq!(got[2].len(), MAX_NAME);
        assert_ne!(got[0], got[1]);
        // Stable across runs.
        assert_eq!(
            got,
            assign(
                &[
                    ("git-hub", "get.issue"),
                    ("git_hub", "get_issue"),
                    ("s", &long)
                ],
                &[]
            )
        );
        assert_ne!(assign(&[("a", "b")], &["mcp__a__b"])[0], "mcp__a__b");
    }
}
