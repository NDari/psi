# Known issues

Problems we know about and have chosen not to fix yet, with how much they matter. When one is fixed, remove it here and say so in the commit.

## Open

### Bash can write what the file tools guard (added 2026-11-03, high priority for the first item)

The file tools ask before changing Psi's configuration or files checkpoints can't undo, but bash only goes through the sandbox and the always-ask list. A heredoc is the easy way (the cog trial on 2026-11-03 wrote an extension with `cat > .psi/extensions/cog_run/init.lua << 'EOF'` after `write` was refused), but `cp`, `sed` or a script do the same.
- **The project's `.psi/` (high).** The sandbox lets bash write the whole workspace, `.psi/` included, so a model can change settings and extensions without asking. An extension's Lua runs inside Psi, outside the sandbox, and loads on the next `/reload` or start: a way out of the sandbox. Fix options: make `<root>/.psi` read-only inside the sandbox (a read-only bind after the workspace bind in bubblewrap, a `file-write*` deny in Seatbelt), as `~/.psi/` already is, so the file tools (which ask) are the only way in; or the plan's checkpoint-diff re-trust (§13), which waits.
- **Always-ask commands in a file (accepted, §8.3).** The classifier reads the command text (with `sh -c`, `eval`, wrappers and substitutions), not files: a script written first and run after (`bash push.sh`, `make deploy`, `npm run x`, `python x.py`) runs what is inside without asking. The plan accepts this ("dynamic forms run; the boundary contains them"). Worth knowing which credentials the sandbox can reach, since that decides what a hidden `git push` can do.
- **Gitignored files (medium).** write and edit ask before changing one, since checkpoints skip it; bash doesn't, so a clobbered `.env` can't be undone.
- **Read before overwrite (low).** The file tools refuse to replace a file the model hasn't read; bash doesn't check. Checkpoints still undo it.

### A TUI test is timing-sensitive under load (added 2026-11-03, low priority)

`undo_diff_show_stats_and_mode` (`crates/psi-cli/tests/tui.rs`) failed once in a full `just check` ("`\"text\": \"one\ntwo\n\"` never reached the pager"; the pager still held `/diff`'s output) and passed alone and in the next full run. It waits for the pager's file with a timeout while the other TUI tests load the machine. Fix: wait for the pager to be given the new output (a count or a marker) rather than a time.

### The user's words are hard to read on a light terminal (added 2026-11-03, low priority)

Committed user messages are bright white and bold (`commit_user` in `crates/psi-cli/src/interactive/ui.rs`, plan §11.2), which stands out on a dark background but nearly disappears on a white one. On a light theme they should be dark instead (black, or the terminal's own foreground, bold). Options: a theme token for the text (pi's themes have `userMessageText`, which Psi uses for the `▌` bar), or picking by the terminal's background (OSC 11 query, or `COLORFGBG`). Low priority: the owner uses a dark terminal.
