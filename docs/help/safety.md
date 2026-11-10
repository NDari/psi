What is refused, what can be undone, and the sandbox.

Nothing asks before a call. Inside a run an action can be undone, or it is refused.

- Checkpoints: psi snapshots the workspace when you send a prompt and after every
  step that may change files, in a shadow git repository (~/.psi/checkpoints). Small
  gitignored files (up to 1 MB, such as .env) are in it too, unless their whole
  directory is ignored (target/, node_modules/). Each snapshot also records the
  project's branch and tag tips and where HEAD points.
- /undo and /rewind restore the files and put those git refs back, so the model may
  commit, branch and rewrite local history. /diff shows the changes. A branch made
  since the snapshot is left alone.
- Sandbox: bash runs in bubblewrap (Linux) or Seatbelt (macOS); it can write the
  workspace, its git directories, cache_dirs and its TMPDIR. On Linux /tmp is the
  session's TMPDIR, so files there last between calls; on macOS /tmp is shared. A
  blocked write fails and the model is told. network: off cuts bash off. If bash can't
  be isolated, bash calls are refused (the status line says BASH OFF).
- Credentials are out of reach: ~/.ssh, ~/.gnupg, git credentials, the gh token, cloud
  and registry keys are empty or unreadable inside the sandbox, read refuses them, and
  bash gets no *_TOKEN, *_KEY or agent socket variables. So git push fails however it
  is written.
- The refuse list: a short list of commands bash never runs, wherever they appear in a
  command: git push, publishing, kubectl, sudo, nested psi runs, and removing or moving
  .git in any form ("rm .git": checkpoints can't undo it). The model is told to leave
  them to you. Add or drop entries with the refuse and refuse_remove settings.
- write and edit only change files inside the workspace and the session's TMPDIR.
- To run something the model is refused, use !cmd: it runs as you, outside the
  sandbox, and the model sees the output.
