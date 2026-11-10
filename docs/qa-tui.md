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
- [ ] BEL rings when a run or a plan ends while the terminal window is unfocused (switch away, then back).
- [ ] In ghostty inside tmux (`focus-events on`, `allow-passthrough on`): switching to another pane during a run gives a desktop notification when it ends; the tab shows progress while working; the pane title reads `ψ <dir> · working`, and the old title is back after quitting. (auto: the sequences)

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
- [ ] `/plan` after talking a change through makes the model write `plan.md` with a checklist; `/run` carries out each open item under a `▌ task n of m` header, shows a report after each (ended, changed, check, counts, ticked or left open) and a `plan:` line at the end; the file's boxes are ticked. Esc during a task stops the plan and leaves the item open. (auto: one item)
- [ ] After a `/run`: `/tasks` lists each task with done or open, files and check; `/diff task 1` pages that task's changes only; `/undo task 1` restores the files and unticks the plan; a number that isn't there says how many tasks there are. (auto)
- [ ] A run that loops (the same calls again and again) goes on once with a note, then stops with `stopped: ...`; nothing asks, and typing "go on" starts a new run.
- [ ] A refused command (ask the model to `git push`) shows `refused: ...` under the call and no card; the run goes on. (auto)
- [ ] A blocked write (try `echo x > /etc/psi-test` in bash via the model) fails with "Blocked: ..." in the result and no card.
- [ ] Inside bash, `ls ~/.ssh` is empty and `git push` fails for lack of credentials; `/undo` after the model commits or rewrites history puts the branch back (`git log` shows the old tip).
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
- [ ] `/undo`, `/diff`, `/stats` work. (auto)
- [ ] The status line shows model, context use, tok/s, time to first token, checkpoints, and `BASH OFF` in red when bubblewrap doesn't work.
- [ ] Against llama-server: `ctx N% of 64K` turns yellow from 70% and red from 90%; after an answer, `read N tok/s` and `cache N%` show; on a long prompt the spinner reads `reading the prompt: N% of ... tokens, ... from the cache` before the answer streams.
- [ ] After quitting, the terminal is back to normal (cursor shown, no raw mode), and psi prints the session file.

## Batteries (Phase 6)

- [ ] With `post_edit_check` set, an edit's result ends with the check's pass line or its first errors.

