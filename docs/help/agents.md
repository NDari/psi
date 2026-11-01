Sub-agents: the model hands a task to another agent with its own context.

Off by default; set "subagents": true in settings to offer the agent tool. A sub-agent
works on the task in a fresh context and returns only its last answer, so side work
(searching, reading many files, a contained change) doesn't fill the main conversation.
It runs one at a time, can't start sub-agents of its own, and asks you for the same
things the main agent would; the approval card names it.

Built in: scout (reads and searches, changes nothing; bash runs read-only commands) and
worker (every tool). Add your own as markdown files in ~/.psi/agents/ or, in a trusted
project, .psi/agents/ (pi's format; a file named like a built-in one replaces it):

  ---
  name: reviewer
  description: Reviews a change and lists problems.
  tools: read, bash
  ---
  You are reviewer. ...

tools lists the tools it may use (default: all). An agent without write and edit only
reads. Each sub-agent's history is its own session file, in <session>.agents/ next to
the main one.
