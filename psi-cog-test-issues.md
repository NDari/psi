# Issues from the cog trial

Problems the owner notices during the long-session trial in `~/projects/cog` (started 2026-11-03), collected as they come up ("add ... to cog") and gone through together later. Each entry: what happened, where it shows, and what might fix it. Fixed entries move to the end with the commit.

## Next

1. **Review the trial's sessions (Claude Code, next session)** and add what they show as entries below. The sessions (the trial ran 2026-11-03, about 16:52 to 18:41 local, the 35B at 64K with `-ub 2048`):
   - `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2252_e48f5e9d.*`: the first extension attempt (`.psi` refusal, the `$TMPDIR` folder), already covered.
   - `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`: the long session (the extensions, the review, the fixes in commits `89eb5d8` and `7feaf2b` in cog), 65 MB of wire log.
   - `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.agents/01-scout.*`: the scout call (about 90 requests).

   What to check: time to first token, reading and generation time, and where the cache broke and why; clearings and summaries, and whether facts survived them; tool errors, loops, re-reads, guard events and pauses; edits that failed and how they recovered; whether the post-edit check, gopls (MCP), the todo list and the extensions were used, and how; claims of checking that no call backs up; anything the owner marked with `/label`. Note what went well too.
2. **Go through every entry here and in `known-issues.md` with the owner**, and decide for each: fix now, later, or drop.

## Open

### `psi.re` has no `escape` (added 2026-11-03)

While building the `cog_run` extension, the model wrote `psi.re.is_match("^" .. psi.re.escape(proj_abs), abs)` to check that a path lies inside the project, and `psi ext check` failed: "psi.re.escape is not part of the API". `psi.re` offers only `is_match`, `find`, `find_all`, `replace` and `split` (`docs/api.md`), so there is no way to put a literal string (a path, user input) into a pattern without escaping it by hand. The model then got stuck: its edits to fix the file were refused (since changed: writes into `.psi` ask).

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2252_e48f5e9d.*`, steps 37 to 40.

Possible fixes:
- Add `psi.re.escape(s)` (the `regex` crate's `regex::escape`), documented and linted like the rest.
- For this case a regex isn't needed at all: `psi.path.relative(abs, proj_abs)` starting with `..` means outside. The docs could show that where paths are compared, so a model reaches for it.

### The extension test API is missing common assertions (added 2026-11-03)

Writing tests for the `protect-words-md` and `test-cmd` extensions, the model called `test.neq` six times; each failed with "attempt to call a nil value (field 'neq')" and it rewrote the tests around it. The only assertions are `test.eq`, `test.truthy` and `test.contains` (`docs/extensions.md`, the test harness in `crates/psi-ext/src/testing.rs`). Models expect the usual set from other test libraries.

Related, from the same tests:
- `test.ui.answer` is documented for confirm, select, input and custom, but `exec` and `http` answers work too (the examples use them); the model found them only by reading `psi ext example changes` and the harness source (`testing.rs`).
- Passing a Lua function where a value is expected (five runs of `test_cmd_test.lua`, the helper at line 6) gave "deserialize error: unsupported value type `function`", which doesn't say which argument or what to pass instead.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`.

Possible fixes:
- Add `test.neq`, `test.falsy`, `test.not_contains`, `test.matches` (a pattern) and `test.errors(fn, text?)` (expects a Lua error), with messages like the existing ones.
- List every `test.ui.answer` kind (confirm, select, input, custom, exec, http) and the shape each takes.
- Name the argument in the conversion error: "test.ui.answer: the value can't be a function; pass a table like { code = 0, stdout = \"...\" }".

### Shift+Enter sends instead of starting a new line (added 2026-11-03)

In the trial, Shift+Enter sent the prompt to the model instead of adding a line. Psi already binds Shift+Enter (and Ctrl+J) to a new line (`crates/psi-tui/src/keys.rs`, `tui.input.newLine`), and asks the terminal to tell modified keys apart (the kitty keyboard protocol, `DISAMBIGUATE_ESCAPE_CODES`). But the owner runs Psi inside tmux 3.8 with `extended-keys off`, so tmux passes Shift+Enter on as a plain Enter and Psi can't see the Shift. Pi warns about exactly this at startup ("tmux extended-keys is off. Modified Enter keys may not work"); Psi says nothing.

Workaround now: Ctrl+J adds a new line in any terminal.

Fix for the owner's setup (`~/.tmux.conf`, then `tmux source-file ~/.tmux.conf` and a new pane):
```
set -g extended-keys on
set -g extended-keys-format csi-u
set -as terminal-features ",xterm-ghostty:extkeys"
```

Possible fixes in Psi:
- At startup inside tmux, read `tmux show -gv extended-keys`; if it is off, say so once with the lines above and that Ctrl+J works meanwhile (pi parity).
- Name Ctrl+J next to Shift+Enter wherever new lines are explained (`/help`, `psi help commands`).

### Scout's 30-request limit can't be changed in settings (added 2026-11-03)

In the trial the model handed scout a broad review of cog ("explore the project and report on any issues": build and test health, dead code, duplication), and scout made about 90 model requests in 4.5 minutes, pausing at 30 and 60 for the owner to go on. The limit is a constant, `MAX_TURNS = 30` in `crates/psi-agent/src/subagent.rs`, used for every sub-agent call (`crates/psi-agent/src/run/sub.rs`); settings only turn sub-agents on (`"subagents": true`). Since 2026-11-03 a budget in the TUI pauses and asks instead of stopping, which is what happened here.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.agents/01-scout.*`.

Also seen: the task asked scout to run `go build`, `go vet` and `go test`, but scout's prompt says it runs only commands that read ("no tests, builds or scripts"), so the parent's task and scout's role disagree; the parent model should have used worker, or run those itself.

Possible fixes:
- A setting for the limit, per agent or for all: `"subagents": {"max_turns": 60}` (keeping `true` working), or a `max_turns` field in an agent's definition file next to `tools` and `model`.
- Or treat sub-agents like the main run in the TUI: no limit by default, the guards and Esc still apply; keep 30 for print runs and evals.
- Say in the agent tool's description that scout only reads and reports, so a task that needs builds or tests goes to worker.
- In auto mode, go on by itself at the limit while the progress guards are quiet (no streak of failed calls, no re-reading loop, no pile of summaries without a file changed), and pause only when one of them has fired; strict mode always pauses. Record each automatic extension as a guard event, so the review can see it. Who approves stays the same: the model can't extend its own limit (a looping model would), and today only the owner's pause card can (`c` continue, `s` stop); auto and strict mode both pause, and headless runs stop.

### Answers look flat: titles run into the next line, headings have no color (added 2026-11-03)

A review answer in the trial (12 findings under `### HIGH`, `### MEDIUM`, `### LOW`) looked like plain text with only the `code` spans colored. The model wrote each finding as a bold title on its own line and the location on the next:

```
**1. `bracketsBalanced` in REPL sends incomplete `[` input**
`cmd/cog/repl.go:92-95` - when a bare `[` (not `#[`) is encountered, ...
```

What the screen does with that, all as CommonMark says:
- **The two lines become one paragraph.** A single newline is a soft break, which renders as a space, so the title, the path and the explanation run together: "1. bracketsBalanced in REPL sends incomplete [ input cmd/cog/repl.go:92-95 - when a bare [ ...". The model meant two lines. Pi's renderer (marked, `breaks` off) joins them the same way.
- **Bold is the only mark on the titles,** and in many terminal fonts bold barely shows (in a copy it doesn't show at all).
- **Headings have no color:** Psi's default `mdHeading` style is plain (`crates/psi-tui/src/theme.rs`), where pi's dark theme uses yellow. Level 3 keeps its `###` (pi parity), so `### HIGH` reads as text.
- **Paths with line numbers** (`cmd/cog/repl.go:92-95`) are code spans, not links.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`.

Possible fixes:
- Render a single newline inside a paragraph as a line break (as chat apps and GitHub comments do). Models write for chat, so this matches what they mean; tables, lists and code are unaffected.
- Give headings a default color from the 16 (yellow, as pi's dark theme), and maybe bold text too, so titles stand out without relying on the font.
- Drop the `#`s for every level and tell levels apart by style (underline for 1, color for all).
- Make `path:line` code spans clickable (OSC 8 `file://` link to the file; the line can't go in the link, plan §15 item 5).

### `/diff` is hard to use in a long session (added 2026-11-03, needs the owner's view of what's wrong)

The owner found `/diff` not right in the trial ("even if some are committed"; not yet clear what the fix is). What it does now (`session_changes` in `crates/psi-agent/src/recovery.rs`): the diff between each pair of Psi's own checkpoints along the session, in order, each labelled by its source ("by edit call 12", "by a restore", "outside the agent"), in the pager. It uses Psi's checkpoint store, not the project's git, so commits don't hide anything: in this session the checkpoints cover all 18 changed files, including everything in the owner's two commits (`89eb5d8` the extensions, `7feaf2b` the fixes). But with 64 checkpoints (63 tool calls and 16 prompts), it is one long list where the same file comes back for every edit.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`; checkpoint store `~/.psi/checkpoints/b03eb31bcd7ed331`.

Possible readings and fixes, to settle together:
- **The whole session at once:** by default, one diff from the session's first checkpoint to now (`git diff --stat` on top, then the diff), with the step-by-step view as `/diff steps`.
- **Since the last commit, or since the last prompt:** `/diff` for what is not committed yet (what you'd review before committing), `/diff last` for the last prompt's changes, `/diff session` for everything.
- **Per file:** `/diff <path>` for one file's changes over the session.
- **Committed vs not:** mark the files already in a commit, so committed and pending work can be told apart.
