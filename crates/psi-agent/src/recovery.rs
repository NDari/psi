//! /undo, /rewind and /diff over a session's checkpoints (psi-plan.md §7.2).

use psi_safety::checkpoint::{Checkpoints, Restore};
use psi_session::{Entry, EntryBody, SessionFile};
use serde_json::json;

/// A checkpoint on the branch, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub entry_id: String,
    pub commit: String,
    pub trigger: String,
    pub tool_call_id: Option<String>,
}

pub fn points(branch: &[&Entry]) -> Vec<Point> {
    branch
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::Checkpoint {
                commit,
                trigger,
                tool_call_id,
            } => Some(Point {
                entry_id: e.id.clone(),
                commit: commit.clone(),
                trigger: trigger.clone().unwrap_or_default(),
                tool_call_id: tool_call_id.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// /undo: restores the files and the project's branch tips to the last
/// prompt submit where either differs from now, so repeating it steps
/// further back. The restore is itself checkpointed, so it can be undone.
/// `None` when there's nothing to undo.
pub fn undo(session: &mut SessionFile, cp: &Checkpoints) -> Result<Option<Restore>, String> {
    let now = cp.capture("before undo").map_err(|e| e.to_string())?.commit;
    let submits: Vec<Point> = points(&session.branch())
        .into_iter()
        .filter(|p| p.trigger == "submit")
        .collect();
    for p in submits.iter().rev() {
        let differs = !cp
            .changes(&p.commit, &now)
            .map_err(|e| e.to_string())?
            .is_empty()
            || cp.refs_differ(&p.commit, &now);
        if differs {
            let r = restore(session, cp, &p.commit, "undo")?;
            return Ok(Some(r));
        }
    }
    Ok(None)
}

/// /rewind: moves the conversation to `entry_id` (the next prompt starts a
/// branch there) and restores the files to the last checkpoint at or
/// before it.
pub fn rewind(
    session: &mut SessionFile,
    cp: &Checkpoints,
    entry_id: &str,
) -> Result<Option<Restore>, String> {
    let entries = session.entries().to_vec();
    let target_branch = psi_session::branch(&entries, Some(entry_id));
    if target_branch.is_empty() {
        return Err(format!("no entry {entry_id} in this session"));
    }
    let commit = points(&target_branch).last().map(|p| p.commit.clone());
    let restored = match commit {
        Some(c) => Some(cp.restore(&c).map_err(|e| e.to_string())?),
        None => None,
    };
    session.set_leaf(entry_id).map_err(|e| e.to_string())?;
    session
        .append(EntryBody::Custom {
            custom_type: "rewind".into(),
            data: json!({"to": entry_id}),
        })
        .map_err(|e| e.to_string())?;
    if let Some(r) = &restored {
        record_restore(session, cp, r)?;
    }
    Ok(restored)
}

fn restore(
    session: &mut SessionFile,
    cp: &Checkpoints,
    commit: &str,
    why: &str,
) -> Result<Restore, String> {
    let r = cp.restore(commit).map_err(|e| e.to_string())?;
    session
        .append(EntryBody::Custom {
            custom_type: why.into(),
            data: json!({"to": commit, "before": r.before}),
        })
        .map_err(|e| e.to_string())?;
    record_restore(session, cp, &r)?;
    Ok(r)
}

fn record_restore(session: &mut SessionFile, cp: &Checkpoints, _r: &Restore) -> Result<(), String> {
    if let Some(head) = cp.head() {
        session
            .append(EntryBody::Checkpoint {
                commit: head,
                trigger: Some("restore".into()),
                tool_call_id: None,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// One source of changes for /diff: what changed between two checkpoints.
#[derive(Debug, Clone)]
pub struct Change {
    /// "by bash call c3", "outside the agent", "by /undo".
    pub source: String,
    pub diff: String,
}

/// The session's changes, attributed by source, from consecutive
/// checkpoints on the branch. Tool calls are numbered in session order.
pub fn session_changes(session: &SessionFile, cp: &Checkpoints) -> Result<Vec<Change>, String> {
    let mut out = Vec::new();
    // The latest assistant message's calls, with their numbers.
    let mut calls: Vec<(String, String, usize)> = Vec::new();
    let mut n = 0;
    let mut prev: Option<String> = None;
    for e in session.branch() {
        if e.tool_calls().next().is_some() {
            calls = e
                .tool_calls()
                .map(|(id, name, _)| {
                    n += 1;
                    (id.to_owned(), name.to_owned(), n)
                })
                .collect();
        }
        let EntryBody::Checkpoint {
            commit,
            trigger,
            tool_call_id,
        } = &e.body
        else {
            continue;
        };
        let Some(from) = prev.replace(commit.clone()) else {
            continue;
        };
        if &from == commit {
            continue;
        }
        let diff = cp.diff(&from, commit).map_err(|e| e.to_string())?;
        if diff.trim().is_empty() {
            continue;
        }
        let source = match (trigger.as_deref(), tool_call_id) {
            (Some("tool"), Some(id)) => match calls.iter().find(|(i, _, _)| i == id) {
                Some((_, name, k)) => format!("by {name} call {k}"),
                None => "by a tool call".into(),
            },
            (Some("restore"), _) => "by a restore (/undo or /rewind)".into(),
            _ => "outside the agent".into(),
        };
        out.push(Change { source, diff });
    }
    Ok(out)
}
