//! Shell completion values that come from the disk (clap_complete's
//! dynamic completion, `COMPLETE=fish psi | source`): help topics, model
//! profiles, this directory's sessions, and eval tasks.
//! Each runs when the shell asks, so it stays cheap and never fails.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::Path;

use clap_complete::engine::CompletionCandidate;
use psi_session::{EntryBody, Role};

fn candidate(value: impl Into<OsString>, help: &str) -> CompletionCandidate {
    let c = CompletionCandidate::new(value);
    if help.is_empty() {
        c
    } else {
        c.help(Some(help.to_owned().into()))
    }
}

/// `psi help <topic>`.
pub fn topics() -> Vec<CompletionCandidate> {
    psi_cli::help::TOPICS
        .iter()
        .map(|(name, text)| candidate(name, text.lines().next().unwrap_or("")))
        .collect()
}

/// `--model`: the profiles in ~/.psi/models.jsonc.
pub fn models() -> Vec<CompletionCandidate> {
    let home = std::env::home_dir();
    let Ok(file) = psi_cli::config::load_models(home.as_deref()) else {
        return Vec::new();
    };
    file.models
        .iter()
        .map(|m| candidate(&m.id, &m.base_url))
        .collect()
}

/// `--session`: this directory's sessions, newest first, with their first
/// prompt.
pub fn sessions() -> Vec<CompletionCandidate> {
    let (Some(home), Ok(cwd)) = (std::env::home_dir(), std::env::current_dir()) else {
        return Vec::new();
    };
    let dir = home.join(".psi/sessions").join(psi_session::cwd_slug(&cwd));
    psi_session::SessionFile::list(&dir)
        .into_iter()
        .rev()
        .map(|p| {
            let prompt = first_prompt(&p).unwrap_or_default();
            candidate(p.into_os_string(), &prompt)
        })
        .collect()
}

/// The first user message of a session, on one line, from its first lines.
fn first_prompt(path: &Path) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    for line in BufReader::new(f).lines().take(50) {
        let Ok(e) = serde_json::from_str::<psi_session::Entry>(&line.ok()?) else {
            continue;
        };
        if let EntryBody::Message {
            role: Role::User,
            content,
            ..
        } = &e.body
        {
            let text = psi_session::text_of(content);
            let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            return Some(one.chars().take(60).collect());
        }
    }
    None
}

/// `psi eval ... --tasks`: quick, all, and the task directories under
/// ./evals/code.
pub fn tasks() -> Vec<CompletionCandidate> {
    let mut out = vec![
        candidate("quick", "the quick set (evals/code/SET)"),
        candidate(
            "trial",
            "longer tasks for trying a model (evals/code/TRIAL)",
        ),
        candidate("all", "every task"),
    ];
    let mut names: Vec<String> = std::fs::read_dir("evals/code")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('_'))
        .collect();
    names.sort();
    out.extend(names.into_iter().map(|n| candidate(n, "")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(c: &[CompletionCandidate]) -> Vec<String> {
        c.iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn topics_come_with_their_summaries() {
        let t = topics();
        assert!(values(&t).contains(&"safety".to_owned()));
        assert!(t.iter().all(|c| c.get_help().is_some()));
    }

    #[test]
    fn a_session_shows_its_first_prompt() {
        let d = tempfile::tempdir().unwrap();
        let mut s = psi_session::SessionFile::create(d.path(), d.path()).unwrap();
        s.append(EntryBody::Message {
            role: Role::User,
            content: vec![psi_session::Block::Text {
                text: "fix the   parser\nplease".into(),
            }],
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        })
        .unwrap();
        assert_eq!(
            first_prompt(s.path()).as_deref(),
            Some("fix the parser please")
        );
    }
}
