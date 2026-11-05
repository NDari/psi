//! Sub-agents (psi-plan.md §10.1, D-41): definitions, the built-in ones,
//! and the `agent` tool's schema. Running one is in `run/sub.rs`.

use psi_ai::{Param, ParamKind, ToolSchema};
use psi_tools::Tool;
use serde_json::json;

/// The tool's name.
pub const TOOL: &str = "agent";

/// Added to every sub-agent's system prompt, before its own.
pub const SUB_NOTE: &str = "You are a sub-agent: another agent gave you this task and sees only your last answer. End with everything it needs.";

/// Asked of a sub-agent that a budget or a guard stopped, after the reason.
pub const REPORT: &str = "You can't call tools any more. Report what you found so far, with the paths and line numbers, and what is left undone.";

/// The most model requests one sub-agent call makes in a print run or an
/// eval before it stops. 100 since a broad scout review in the cog trial
/// needed 134 (2026-11-03). In the TUI a sub-agent has no limit, like the
/// main run: worker paused six times at 100 in the second trial
/// (2026-11-04), each waiting on the user.
pub const MAX_TURNS: u32 = 100;

/// One agent: a name, when to use it, its tools and its instructions
/// (pi's format: markdown with `name`, `description` and `tools`
/// frontmatter, the body as the prompt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    /// Tool names it may use; `None`: every tool the session has.
    pub tools: Option<Vec<String>>,
    /// Added to the system prompt.
    pub prompt: String,
}

impl AgentDef {
    /// Whether it may use the tool named `name`.
    pub fn allows(&self, name: &str) -> bool {
        self.tools
            .as_ref()
            .is_none_or(|t| t.iter().any(|n| n == name))
    }

    /// The core tools it gets: those of the session's it allows.
    pub fn core_tools(&self, session: &[Tool]) -> Vec<Tool> {
        session
            .iter()
            .copied()
            .filter(|t| self.allows(t.name()))
            .collect()
    }

    /// Without write or edit it only reads: bash runs read-only commands,
    /// and the parent takes no checkpoint after it.
    pub fn read_only(&self, session: &[Tool]) -> bool {
        !self
            .core_tools(session)
            .iter()
            .any(|t| matches!(t, Tool::Write | Tool::Edit))
    }
}

/// `scout` and `worker`; files of the same name replace them.
pub fn built_in() -> Vec<AgentDef> {
    vec![
        AgentDef {
            name: "scout".into(),
            description: "Explores the code and reports; changes nothing and runs no builds or tests (give those to worker).".into(),
            tools: Some(
                ["read", "bash", "grep", "find", "ls"]
                    .map(str::to_owned)
                    .to_vec(),
            ),
            prompt: "You are scout. Find what the task asks about: search with grep or find, then read the parts that matter, not whole files. You change nothing, and bash runs only commands that read, like grep, cat or git log: no tests, builds or scripts. End with your findings: the files and line numbers that matter, the key code quoted briefly, and how the pieces connect. The agent reading your answer hasn't seen these files.".into(),
        },
        AgentDef {
            name: "worker".into(),
            description: "Carries out a contained change with every tool.".into(),
            tools: None,
            prompt: "You are worker. Carry out the task you were given and keep to it. End with what you did: the files you changed and how, what you checked, and anything left open.".into(),
        },
    ]
}

/// Puts `extra` (files) before the built-in agents: the first of a name
/// wins.
pub fn with_built_in(extra: Vec<AgentDef>) -> Vec<AgentDef> {
    let mut all = extra;
    for d in built_in() {
        if !all.iter().any(|a| a.name == d.name) {
            all.push(d);
        }
    }
    all
}

/// The `agent` tool, listing the agents.
pub fn schema(defs: &[AgentDef]) -> ToolSchema {
    let list: Vec<String> = defs
        .iter()
        .map(|d| format!("- {}: {}", d.name, d.description))
        .collect();
    ToolSchema {
        name: TOOL,
        description: format!(
            "Hand a task to a sub-agent. It works in its own context and returns only its final answer, so use it for side work that would fill your context: searching or reading many files, or a contained change. It sees none of this conversation: put everything it needs in the task.\nAgents:\n{}",
            list.join("\n")
        ),
        params: vec![
            Param::new("agent", ParamKind::String, "The agent's name"),
            Param::new(
                "task",
                ParamKind::String,
                "The whole task, with the paths and facts it needs",
            ),
        ],
        example: json!({"agent": "scout", "task": "Find where request timeouts are set and report the files and lines."}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_follow_the_list_and_the_session() {
        let session = [Tool::Read, Tool::Write, Tool::Edit, Tool::Bash];
        let scout = &built_in()[0];
        assert_eq!(scout.core_tools(&session), [Tool::Read, Tool::Bash]);
        assert!(scout.read_only(&session));
        let worker = &built_in()[1];
        assert_eq!(worker.core_tools(&session), session);
        assert!(!worker.read_only(&session));
    }

    #[test]
    fn files_replace_built_in_agents_of_the_same_name() {
        let mine = AgentDef {
            name: "scout".into(),
            description: "mine".into(),
            tools: None,
            prompt: String::new(),
        };
        let all = with_built_in(vec![mine]);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].description, "mine");
        assert_eq!(all[1].name, "worker");
    }

    #[test]
    fn the_schema_lists_the_agents_and_names_the_agent_first() {
        let v = schema(&built_in()).to_openai();
        let d = v["function"]["description"].as_str().unwrap();
        assert!(d.contains("- scout: Explores"), "{d}");
        assert!(d.contains("- worker: Carries"), "{d}");
        let keys: Vec<&String> = v["function"]["parameters"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(keys, ["agent", "task"]);
    }
}
