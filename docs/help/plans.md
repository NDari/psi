Plans: a checklist the harness carries out, one small task at a time.

How to work with one:
1. Plan with the model in an ordinary conversation.
2. /plan [file] has the model write the plan down (default plan.md): the goal and what a
   fresh reader needs, then a checklist, one step per line as "- [ ] ...".
3. Read it, edit it, then /run [file] in the TUI, or psi run [file] unattended.

What a run does, for each open item in order:
- A fresh context: the model gets the plan, the item, and psi's own record of the items
  done before (what changed, whether the check passed). Nothing of the conversation or
  of earlier tasks is carried over in the model's words.
- When the task ends by itself, psi runs the project's check (the post_edit_check
  setting) if one is set.
- A report, written by psi from its checkpoints and records: how the task ended, the
  files changed, whether git refs moved, the check's result, requests and tool calls.
- psi ticks the item in the file when the task ended by itself and the check passed.
  Otherwise the item stays open, the report says why, and the run stops there. Run it
  again to retry that item; /undo takes its changes back first if you want.

Afterwards: /tasks lists the session's tasks; /diff task <n> shows what one changed;
/undo task <n> puts files, git refs and the plan's ticks back to before task n (and so
the tasks after it). Plain /undo steps back one task at a time.

Limits: each task gets the "limits" setting (requests, tool_calls, summaries). A task
that fills the context window more often than "summaries" allows is too big: split the
item. A run never waits for an answer; at a limit it stops with the reason.

A plan is any markdown file (lines inside code fences are not items). Edit it between
runs: add or reword items, untick one to redo it. psi run exits 0 when all are done.
