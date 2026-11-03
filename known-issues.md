# Known issues

Problems we know about and have chosen not to fix yet, with how much they matter. When one is fixed, remove it here and say so in the commit.

## Open

### The user's words are hard to read on a light terminal (added 2026-11-03, low priority)

Committed user messages are bright white and bold (`commit_user` in `crates/psi-cli/src/interactive/ui.rs`, plan §11.2), which stands out on a dark background but nearly disappears on a white one. On a light theme they should be dark instead (black, or the terminal's own foreground, bold). Options: a theme token for the text (pi's themes have `userMessageText`, which Psi uses for the `▌` bar), or picking by the terminal's background (OSC 11 query, or `COLORFGBG`). Low priority: the owner uses a dark terminal.
