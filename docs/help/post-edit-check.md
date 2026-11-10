The post-edit check: the project's check, run after the model edits files.

Set post_edit_check in the project's .psi/settings.jsonc to a command, or to
{ "command": "cargo check -q", "timeout": 120 }. After each turn in which write or edit
changed files, psi runs it once (inside the sandbox) and adds a short result to the
last edit's result: a pass line, or the first 20 lines of output and the exit code.
When a task of a plan ends by itself, psi runs the check once more, and ticks the item
only if it passes (psi help plans).
