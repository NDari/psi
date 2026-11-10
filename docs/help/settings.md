Settings files and what they hold.

Layers, lowest first: built-in, ~/.psi/settings.jsonc, <project>/.psi/settings.jsonc,
then command-line flags. JSONC: comments and trailing commas are fine. Objects merge by
key; other values replace; refuse lists append. A project's file loads without asking,
because it can't loosen safety: network, isolation, cache_dirs and refuse_remove in it
are ignored (psi says so at startup).

Keys:
- network: "on" or "off". isolation: "on" (bubblewrap on Linux, Seatbelt on macOS) or "off".
- cache_dirs: directories bash may write under the sandbox.
- refuse, refuse_remove: commands bash never runs, added or dropped (psi help safety).
- post_edit_check: a command run after turns that edit files (psi help post-edit-check),
  and at the end of each task of a plan.
- limits: {"requests": 40, "tool_calls": 200, "summaries": 2}: what one task of a plan
  or one print run (-p) may use before it stops with the reason (psi help plans).
  "summaries" also holds in the TUI's chat: a run that fills the context window once
  more stops and says the work is too big for one run.
- notify: "auto" (default), "bell" or "off". When a run or a plan ends while the
  terminal is unfocused, auto rings the bell and sends a desktop notification.
  Psi also shows working in the title and the tab's progress indicator. In tmux,
  set -g focus-events on (to know about focus) and set -g allow-passthrough on.

Models are in ~/.psi/models.jsonc (psi help models).
