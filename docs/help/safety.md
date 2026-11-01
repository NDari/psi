What asks, what can be undone, and the sandbox.

- Checkpoints: psi snapshots the workspace when you send a prompt and after every
  step that may change files, in a shadow git repository (~/.psi/checkpoints). /undo
  and /rewind restore files; /diff shows the changes. The project's own .git is untouched.
- Modes: "auto" asks only for the always-ask list and writes git can't undo
  (gitignored files); "strict" asks before every change. /mode or --mode switches.
- Always-ask: irreversible commands such as git push or publishing ask in every mode,
  wherever they appear in a command, and so does removing or moving .git in any form
  ("rm .git": checkpoints can't undo it). Add or remove entries with always_ask settings.
- Sandbox: bash runs in bubblewrap; it can write the workspace, its git directories,
  cache_dirs and its TMPDIR, and /tmp is private to each call. A blocked write offers to
  rerun outside the sandbox or to make the directory writable. network: off cuts bash off.
- Headless runs (-p) never ask: what would ask is denied, unless --allow <entry> allows it.
- psi refuses file-tool writes to .psi/ directories, so a model can't change its own settings.
