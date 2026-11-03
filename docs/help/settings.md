Settings files and what they hold.

Layers, lowest first: built-in, ~/.psi/settings.jsonc, <project>/.psi/settings.jsonc
(once you trust the project), then command-line flags. JSONC: comments and trailing
commas are fine. Objects merge by key; other values replace; always_ask lists append.

Keys:
- mode: "auto" (default) or "strict" (psi help safety).
- network: "on" or "off". isolation: "on" (bubblewrap on Linux, Seatbelt on macOS) or "off".
- cache_dirs: directories bash may write under the sandbox.
- always_ask, always_ask_remove: commands that always ask, added or dropped.
- post_edit_check: a command run after turns that edit files (psi help post-edit-check).
- todo: true offers the model the todo tool (psi help todo).
- subagents: true offers the model the agent tool (psi help agents).
- compaction: {"tail": tokens kept verbatim after a summary (default "auto"), "summary":
  "structured" (default) or "free"}. For experiments (H-07, H-11).
- theme: a theme name (psi help themes).
- notify: "auto" (default), "bell" or "off". When a turn ends or something waits for you
  while the terminal is unfocused, auto rings the bell and sends a desktop notification.
  Psi also shows working or waiting in the title and the tab's progress indicator. In tmux,
  set -g focus-events on (to know about focus) and set -g allow-passthrough on.
- skills, prompts: more directories of skills or prompt templates.
- extensions: per extension, by name. {"<name>": {"unsandboxed": true}} lets its psi.exec
  commands run outside the sandbox (they still ask for always-ask commands).
- packages: installed packages (psi help packages).

Models are in ~/.psi/models.jsonc (psi help models).
