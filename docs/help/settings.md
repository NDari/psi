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
- post_edit_check: a command run after turns that edit files (psi help post-edit-check).
- compaction: {"tail": tokens kept verbatim after a summary (default "auto": 10K, less in
  small windows), "summary":
  "structured" (default) or "free"}. For experiments (H-07, H-11).
- notify: "auto" (default), "bell" or "off". When a turn ends or something waits for you
  while the terminal is unfocused, auto rings the bell and sends a desktop notification.
  Psi also shows working or waiting in the title and the tab's progress indicator. In tmux,
  set -g focus-events on (to know about focus) and set -g allow-passthrough on.

Models are in ~/.psi/models.jsonc (psi help models).
