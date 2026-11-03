# Interactive mode: manual QA checklist

Run `psi` in a project in your usual terminal (and once inside tmux).
Items marked (auto) are also covered by `crates/psi-cli/tests/tui.rs`,
which drives the real binary in a pseudo-terminal; they are still worth a
look in a real terminal, since the test terminal (vt100) is simpler than
yours.

## Start and feel (§11.3)

- [ ] Startup: earlier terminal content scrolls into scrollback instead of being erased; the banner and input box appear at once. (auto: under 100 ms in a release build)
- [ ] Typing is instant, also while the model streams. (auto: keystroke under 16 ms)
- [ ] Streaming text appears paragraph by paragraph, with no flicker.
- [ ] A long session (scroll back through a few thousand lines) has no input lag.
- [ ] `PSI_DEBUG_RENDER=1 psi` shows the last frame's draw time under the input box.

## Commit-once rules (§11.2)

- [ ] A tool call is committed once: header, a one-line summary, up to 4 output lines, and up to 20 diff lines for an edit. (auto)
- [ ] `/show <n>` prints the whole call. (auto)
- [ ] Code blocks keep their fence lines (dimmed); copying a block from the terminal gives the exact code.
- [ ] `/copy` and `/copy code` put the last answer or its last code block on the system clipboard, also inside tmux. (auto: the sequence)
- [ ] Ctrl+click (ghostty) on a read/edit/write path opens the file; on a markdown link opens the URL; also inside tmux with `terminal-features ',*:hyperlinks'`. (auto: the sequences)
- [ ] Thinking is never shown.
- [ ] BEL rings when a run ends or an approval appears while the terminal window is unfocused (switch away, then back).
- [ ] In ghostty inside tmux (`focus-events on`, `allow-passthrough on`): switching to another pane during a run gives a desktop notification when it ends or an approval appears; the tab shows progress while working; the pane title reads `ψ <dir> · working` / `· waiting`, and the old title is back after quitting. (auto: the sequences)

## Input

- [ ] Shift+Enter (in a terminal with the kitty keyboard protocol: kitty, foot, wezterm, ghostty) and a trailing `\` insert a newline.
- [ ] Up and Down browse history at the first and last line; the unsent draft comes back.
- [ ] Bracketed paste of many lines lands as one draft, not as several prompts. Over 10 lines or 1,000 characters it shows as `[Pasted text #1 +N lines]` and is sent whole; invisible tag and bidi characters are removed with a note. (auto)
- [ ] Ctrl+C clears the input; on an empty input it asks, and a second Ctrl+C quits. Ctrl+D on an empty input quits.
- [ ] Ctrl+R lists earlier prompts newest first; typing filters; Enter puts one in the box without sending; Esc leaves the box as it was. Ctrl+S stashes the draft and brings it back. (auto)
- [ ] Ctrl+G opens the draft in `$VISUAL`/`$EDITOR` (nvim); saving and quitting puts the edited text back; the screen is intact afterwards. (auto: with a script as the editor)

## Dropdowns and modals (they go under the input box, never in its place)

- [ ] `/` at the start lists commands; Up/Down put the selection in the box (what the box shows is what Enter runs); Tab accepts; Esc closes. (auto)
- [ ] `@` lists files fuzzily, .gitignore-aware; Tab inserts the path. (auto)
- [ ] The approval card appears under the input box and the draft stays; `y`, `s`, `n`, and `e` (type a message, Enter) all work. (auto: n)
- [ ] A blocked write (try `echo x > /etc/psi-test` in bash via the model) shows the blocked card: `r`, `a`, `n`.
- [ ] `/rewind` and `/tree` pickers: Up/Down, Enter, Esc. (auto: rewind)
- [ ] `/resume` lists this directory's sessions and opens one, replaying its messages.

## Resize

- [ ] Making the window wider or shorter redraws only the input area; committed lines stay as they were. (auto: wider and shorter)
- [ ] Making it narrower: terminals that reflow (kitty, foot, alacritty, wezterm, GNOME Terminal, Konsole) keep committed lines intact. On one that doesn't (xterm), a few committed rows just above the input may be cleared: known limitation.
- [ ] Shrinking the height below the input box and growing it again leaves a clean input box.

## Runs

- [ ] Enter while running steers the run; Alt+Enter queues a follow-up for when it ends. Both show as `(steering)` / `(follow-up)` until the agent takes them. (auto: steering)
- [ ] Esc aborts the request and running tools. (auto)
- [ ] `!cmd` runs a command as you; its output goes to the model with the next prompt. `!!cmd` doesn't send it. (auto: `!`)
- [ ] `/undo`, `/diff`, `/stats`, `/mode strict` (the status line shows strict) work. (auto)
- [ ] The status line shows model, context use, tok/s, time to first token, mode, checkpoints, and `UNISOLATED` in red when bubblewrap doesn't work.
- [ ] After quitting, the terminal is back to normal (cursor shown, no raw mode), and psi prints the session file.

## Batteries (Phase 6)

- [ ] `/plan` (or Ctrl+Alt+P) shows `plan` in the status line; the model can't write or edit, and a bash command that isn't read-only is refused. (auto)
- [ ] An answer with a `Plan:` list shows the steps and asks what next; "Carry out the plan" turns plan mode off, and the status line counts `[DONE:n]` steps with a list of them above it. (auto)
- [ ] `/todos` shows the todo list for the current branch (with `"todo": true` in settings); after `/tree` to an earlier prompt it shows that point's list.
- [ ] `/<template> args` and `/skill:name args` complete in the dropdown and run; a skill message shows as `[skill name]`. (auto)
- [ ] `~/.psi/keybindings.json` rebinds keys; a bad entry is a startup warning and keeps the default; `/help` lists the keys in use. (auto)
- [ ] `/theme` lists the themes; `/theme <name>` recolors what is drawn after it; a theme with truecolor values looks right in a truecolor terminal. (auto: a 256-color index)
- [ ] With `post_edit_check` set, an edit's result ends with the check's pass line or its first errors.

