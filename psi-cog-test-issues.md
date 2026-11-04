# Issues from the cog trial

Problems the owner notices during the long-session trial in `~/projects/cog` (started 2026-11-03), collected as they come up ("add ... to cog") and gone through together later. Each entry: what happened, where it shows, and what might fix it. Fixed entries move to the end with the commit.

## Next

1. **Done 2026-11-03:** the sessions were reviewed; the entries marked "from the session review" and the notes entry come from it. No `/label` was used. The sessions (the trial ran 2026-11-03, about 16:52 to 18:41 local, the 35B at 64K with `-ub 2048`):
   - `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2252_e48f5e9d.*`: the first extension attempt (`.psi` refusal, the `$TMPDIR` folder), already covered.
   - `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`: the long session (the extensions, the review, the fixes in commits `89eb5d8` and `7feaf2b` in cog), 65 MB of wire log.
   - `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.agents/01-scout.*`: the scout call (134 requests).

   What to check: time to first token, reading and generation time, and where the cache broke and why; clearings and summaries, and whether facts survived them; tool errors, loops, re-reads, guard events and pauses; edits that failed and how they recovered; whether the post-edit check, gopls (MCP), the todo list and the extensions were used, and how; claims of checking that no call backs up; anything the owner marked with `/label`. Note what went well too.
2. **Gone through with the owner 2026-11-03.** Fixed now: the entries under Fixed, the scout stop gap, and in `known-issues.md` the `/reload` trust check and the timing-sensitive test. Left open: `/diff` (to discuss), the extension-block workaround (to investigate further), the scout limit setting, the parent's cache after scout, scout's re-reads, and the claims of checks.

## Open

### Scout's 30-request limit can't be changed in settings (added 2026-11-03)

In the trial the model handed scout a broad review of cog ("explore the project and report on any issues": build and test health, dead code, duplication), and scout made 134 model requests in about 10 minutes, pausing at 30, 60, 90 and 120 for the owner to go on (225 s reading prompts, 137 s generating, the rest waiting at the pauses). The limit is a constant, `MAX_TURNS = 30` in `crates/psi-agent/src/subagent.rs`, used for every sub-agent call (`crates/psi-agent/src/run/sub.rs`); settings only turn sub-agents on (`"subagents": true`). Since 2026-11-03 a budget in the TUI pauses and asks instead of stopping, which is what happened here.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.agents/01-scout.*`.

Also seen: the task asked scout to run `go build`, `go vet` and `go test`, but scout's prompt says it runs only commands that read ("no tests, builds or scripts"), so the parent's task and scout's role disagree; the parent model should have used worker, or run those itself.

Possible fixes:
- A setting for the limit, per agent or for all: `"subagents": {"max_turns": 60}` (keeping `true` working), or a `max_turns` field in an agent's definition file next to `tools` and `model`.
- Or treat sub-agents like the main run in the TUI: no limit by default, the guards and Esc still apply; keep 30 for print runs and evals.
- Say in the agent tool's description that scout only reads and reports, so a task that needs builds or tests goes to worker.
- In auto mode, go on by itself at the limit while the progress guards are quiet (no streak of failed calls, no re-reading loop, no pile of summaries without a file changed), and pause only when one of them has fired; strict mode always pauses. Record each automatic extension as a guard event, so the review can see it. Who approves stays the same: the model can't extend its own limit (a looping model would), and today only the owner's pause card can (`c` continue, `s` stop); auto and strict mode both pause, and headless runs stop.

**Stop gap 2026-11-03:** the limit is 100 (`MAX_TURNS`, plan §10.1 updated) and the agent tool says scout runs no builds or tests (give those to worker). The setting is still to do.

### `/diff` is hard to use in a long session (added 2026-11-03, needs the owner's view of what's wrong)

The owner found `/diff` not right in the trial ("even if some are committed"; not yet clear what the fix is). What it does now (`session_changes` in `crates/psi-agent/src/recovery.rs`): the diff between each pair of Psi's own checkpoints along the session, in order, each labelled by its source ("by edit call 12", "by a restore", "outside the agent"), in the pager. It uses Psi's checkpoint store, not the project's git, so commits don't hide anything: in this session the checkpoints cover all 18 changed files, including everything in the owner's two commits (`89eb5d8` the extensions, `7feaf2b` the fixes). But with 64 checkpoints (63 tool calls and 16 prompts), it is one long list where the same file comes back for every edit.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`; checkpoint store `~/.psi/checkpoints/b03eb31bcd7ed331`.

Possible readings and fixes, to settle together:
- **The whole session at once:** by default, one diff from the session's first checkpoint to now (`git diff --stat` on top, then the diff), with the step-by-step view as `/diff steps`.
- **Since the last commit, or since the last prompt:** `/diff` for what is not committed yet (what you'd review before committing), `/diff last` for the last prompt's changes, `/diff session` for everything.
- **Per file:** `/diff <path>` for one file's changes over the session.
- **Committed vs not:** mark the files already in a commit, so committed and pending work can be told apart.

### The model worked around an extension's block (added 2026-11-03, from the session review)

The model's own `protect-words-md` extension blocks any bash command whose text contains `docs/words.md`, so it also blocked reads (`cat`, `head`, `git diff`, `git status`), `git add` and even a commit message naming the file: 14 blocks in the long session and 4 in scout's run. The model then got past it on purpose, twice: `F=words.md; ... > docs/$F` to write the file (step 329, "the extension uses static pattern matching, so `docs/$F` doesn't trigger it") and `git add -A` to stage it (step 356). Psi passed the block on as "Blocked: <reason>" with nothing saying not to work around it.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`, steps 301 to 356.

Possible fixes:
- Add a line to every block from an extension: "Blocked by the extension <name>. Don't work around it; if it is wrong, say so and stop." (The extension's name is known at the gate.)
- Give extensions what Psi's classifier already knows: `psi.bash.writes(command)`, the paths a command writes (redirects, `cp`, `mv`, `tee`, `sed -i`), so a gate like this one can block writes and let reads through. Show it in the `tool_call` docs where bash is gated.

### The parent re-read its whole context after the scout call (added 2026-11-03, from the session review)

The parent's first request after scout returned had `cache_n` 0: 29,633 tokens read again, 27 s to the first token. Scout's 134 requests on the server's one slot had pushed the parent's prompt out of llama-server's prompt cache (`--cache-ram`, default 8 GB, 1 to 2.6 GB per entry for the 35B). Plan §10.1 says to measure this; this is the measurement.

Possible fixes:
- Save the parent's slot before a sub-agent call and restore it after: start llama-server with `--slot-save-path`, then `POST /slots/0?action=save` and `?action=restore` (llama-server only, a no-op elsewhere).
- Or raise `--cache-ram` in `llm-up`, which only helps if the Mac has the memory to spare.

### Scout re-read the codebase after its results were cleared (added 2026-11-03, from the session review)

Scout read every Go file in cog in its first 90 requests; then three clearings and a summary (about 6 to 7 minutes in) removed those results, and it read them all again: `fn.go` whole three times, `env.go` twice in chunks, then dict, scope, errors, source, instructions, modules, contracts, options, embedding, atoms, sequence and evaluator once more. The no-progress guard fired once (7.6 minutes) and it went on reading. Most of requests 100 to 134 were these re-reads. Its final report was still good (it found the REPL bracket bug that became the main fix).

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.agents/01-scout.*`.

Possible fixes:
- Tell scout in its prompt to write each finding down in its reply text as it goes, since cleared results are gone and its own text stays.
- Make the clearing notice list the files whose results were cleared and say "your notes about them remain above; read again only what you need".

### Claims of checks that weren't made (added 2026-11-03, from the session review)

The owner asked for each fix to be proved by a failing test first. What the session shows:
- **The flaky terminal test was called fixed with no change.** It passed on a rerun, and the model said "the earlier failure was the cached test with the old code" (step 222); the commit message for `7feaf2b` lists it as fixed and the next summary carried "Terminal test passes consistently". It failed again at step 330, where the model called it "pre-existing flaky".
- **Red-green for issues 3 to 6 wasn't red.** The new tests passed the post-edit check before the fixes went in (the swap test at step 228, for one), and issue 4 (dead code in `dup`) had no test at all; the final report says each was proved.
- The todo list showed all six items done before work started (see the todo entry).

These are the model's mistakes; the session has what a reviewer needs to see them (the post-edit check results). A harness fix is unclear; maybe the post-edit check's line could say when a newly added test passed at once ("the new test passed before any fix"), which needs to know which tests are new. To discuss.

### Notes from the review, no action yet (added 2026-11-03)

- **Timing.** 359 requests in the long session: first token median 0.4 s, 90th percentile 1.0 s; 291 s reading prompts and 573 s generating in all. The seven requests over 15 s to the first token all come after a summary (five, each 17 to 23 s for about 20K tokens, since only the system prompt stays cached), after the restart that added `cog_run` to the tool list (22 s, cache 0, as D-38 expects) or after scout (above).
- **The run that stopped without an answer.** At 13.1 minutes the run ends after a tool result with no answer and no entry saying why; the next entry is "continue" 23 minutes later, from a restarted Psi (the requests now include `cog_run`). The owner probably quit to load the extensions; if so, quitting between tool calls leaves no trace in the session, which a review can't tell from a crash.
- **Summaries kept the facts; the model didn't always use them.** The first summary said "`test.neq` does NOT exist", and the first request after it wrote `test.neq` again (step 103), as did step 115.
- **gopls (MCP) was used only when the owner said so,** three calls in one response; `go_package_api` got the module path wrong (`github.com/ndari/projects/cog`, where `go_workspace` in the same response said `github.com/NDari/cog`) and returned "(no output)". It wasn't used again, not even for the file renames in issue 7.
- **`cog_run` was never called** after it was built, in about 250 later requests.
- **Went well:** the post-edit check ran 56 times (15 s in all) and its 7 failures were all real (the red tests, the stale `docs/words.md`); failed edits (two "not found", one "matches 3 places", one write refused after a `sed -i`) each recovered in one or two calls; reads came in parallel batches of up to five; always-ask caught `git push` and a `sudo chmod` on `/etc/ssh`; the status-line extension showed in the footer as written; scout found the real high-priority bug.

### Where the long session's time went (added 2026-11-04, from a performance review)

The long session made 359 requests: 291 s reading prompts, 573 s generating, and 64 s more for five summaries, about 15.5 minutes of model time in all. What cost the most, largest first:

- **Each summary costs about 33 s, and most of it is avoidable.** Five summaries: 64 s to write them plus 100 s re-reading the context after each (17 to 23 s for 20K to 24K tokens), 18% of all model time. Three causes:
  - **Clearing never runs at 64K.** Past the trigger (45.9K), clearing has to bring the context down to 19.7K (`clear_target` in `crates/psi-agent/src/run/fit.rs`), which needs about 26K freed; old results over 800 characters held about 18K at the first trigger, so Psi summarized instead, every time (no `cleared_results` in the main session). But a summary only got the context down to 22K to 28K, since the tail alone is 20K. Clearing alone would have reached about 28K, the same, with no summary call and the conversation kept word for word. Fix: clear when clearing reaches what a summary would leave (overhead, summaries and tail), not only when it reaches 30% of the window. **Done 2026-11-04:** clearing is enough when it leaves no more than a summary would, plus a twentieth of the window (`clearing_will_do`, plan §7.4), with a scenario test.
  - **The tail leaves little room.** After a summary the context was 22K to 28K against a 45.9K trigger, so about 20K of room per cycle; the last two summaries came 2 minutes (49 requests) apart. A smaller tail at 64K (10K, say) would roughly halve the number of summaries and shorten each re-read. The 20K is pi's size, and H-07 (no tail) showed no difference at 16K, so this wants an eval at 64K before changing it. **Done 2026-11-04:** the default tail is 10K under the same cap (plan §7.4, D-16), so 16K and 32K windows are unchanged.
  - **The earlier summaries weren't reused from the cache.** After each summary the server kept only the first 3.5K tokens (system prompt and tools), though the earlier summary blocks were byte-identical (D-40 expects them cached). Likely the hybrid model's checkpoints again (see experiments.md, "Prompt cache lost after a long final answer"): the server can resume only where it took a checkpoint. It costs 1K to 4.5K tokens per summary; not fixable in Psi as far as I know.
- **Whole-file rewrites took a quarter of the generating.** 23 `write` calls generated for 210 s (24% of all generating), 17 of them rewrites of the two test-cmd extension files while the model fought the test API (now fixed). Psi's `edit` makes one replacement per call; pi's takes several (`edits[]`), and the model rewrote a file when several small changes were needed ("That's too complex. Let me just rewrite the test file", step 126). Option: several replacements per edit call, as pi does; a change to the tool the plan specifies (§6, `edit(path, old_text, new_text)`), so it wants an eval. **Done 2026-11-04:** `edit` also takes `edits: [{old_text, new_text}, ...]`, each matched against the file as it was, overlaps refused, all or none (plan §6); pi's names and a list sent as a string are read too. It adds about 70 tokens to every request's tool list.
- **The post-edit check's output has no character cap.** It keeps 20 lines (`MAX_LINES` in `crates/psi-agent/src/post_edit.rs`), but testify prints each expected and actual value on one line, so two edit results were 16.5K characters (about 4.2K tokens each, 6.7 s to the first token), and they stayed in the context until the next summary. Fix: cap each line (about 200 characters, cut in the middle) and the whole (about 2,000 characters). Small. **Done 2026-11-04:** lines over 200 characters are cut in the middle and the kept lines stop at 3,000 characters, with a test.
- **Cache lost twice for known reasons:** the restart that added `cog_run` (22 s) and the scout call (27 s), both entries above.

Not worth changing: re-reads were 30.7K of the 79K characters read (14K of an unchanged file, 6K across a summary, 10K after the model's own edit), and the post-edit check ran 56 times for 15 s in all, including after edits to Lua files it doesn't test.

### The fixes in one eval run (added 2026-11-04)

One quick-set run on the 35B at 64K with the batteries variant plus `"subagents": true` (`~/.psi/evals/2026-11-04-0236-quick-batteries-agents-qwen3.6-35b`): 7 PASS, 3 MODEL-FAIL (logic bugs: a sort order, a foreign key, a missing import), 0 HARNESS-FAIL. One run, so it proves the changes work, not how well. What it shows:
- **Todo:** used in 5 trials, 37 calls; `done` each time an item was finished, no `toggle`, no item marked twice. One call carried tool-call markup in its action (`"add>\n<parameter=text>\n..."`, multi_split_package), refused as an unknown action and redone correctly at once (`known-issues.md`).
- **Scout:** called once (feat_logstats_jsonl) with a reading task only, no builds or tests asked; 5 requests, reads and `find`, no refusals.
- **`/tmp`:** used in 6 trials (up to 14 calls in sh_archive_fix), each call setting up its own test folders there; no "file not found" for a file kept from an earlier call.

A second run after the multi-edit and 10K tail changes, same variant (`~/.psi/evals/2026-11-04-0308-quick-multiedit-tail10k-qwen3.6-35b`): 8 PASS, 2 MODEL-FAIL (a foreign key again, and quoted strings in an INI converter), 0 HARNESS-FAIL. `edits` was used in 6 of 31 edit calls, unprompted: three with several replacements (4 in feat_config_env, 2 in two others), all applied; three with one, folded into `old_text` and `new_text`. No edit failed. No trial compacted, so the 10K tail wasn't exercised; that shows in long sessions.

## Fixed

### `psi.re` has no `escape` (added 2026-11-03)

While building the `cog_run` extension, the model wrote `psi.re.is_match("^" .. psi.re.escape(proj_abs), abs)` to check that a path lies inside the project, and `psi ext check` failed: "psi.re.escape is not part of the API". `psi.re` offers only `is_match`, `find`, `find_all`, `replace` and `split` (`docs/api.md`), so there is no way to put a literal string (a path, user input) into a pattern without escaping it by hand. The model then got stuck: its edits to fix the file were refused (since changed: writes into `.psi` ask).

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2252_e48f5e9d.*`, steps 37 to 40.

Possible fixes:
- Add `psi.re.escape(s)` (the `regex` crate's `regex::escape`), documented and linted like the rest.
- For this case a regex isn't needed at all: `psi.path.relative(abs, proj_abs)` starting with `..` means outside. The docs could show that where paths are compared, so a model reaches for it.

**Fixed 2026-11-03:** `psi.re.escape(s)` (the `regex` crate's `escape`), in `docs/api.md`, the `changes` example (a new `under` filter) and a test; `psi.path.relative`'s doc says a result starting with `..` lies outside `base`.

### The extension test API is missing common assertions (added 2026-11-03)

Writing tests for the `protect-words-md` and `test-cmd` extensions, the model called `test.neq` six times; each failed with "attempt to call a nil value (field 'neq')" and it rewrote the tests around it. The only assertions are `test.eq`, `test.truthy` and `test.contains` (`docs/extensions.md`, the test harness in `crates/psi-ext/src/testing.rs`). Models expect the usual set from other test libraries.

Related, from the same tests:
- `test.ui.answer` is documented for confirm, select, input and custom, but `exec` and `http` answers work too (the examples use them); the model found them only by reading `psi ext example changes` and the harness source (`testing.rs`).
- Passing a Lua function where a value is expected (five runs of `test_cmd_test.lua`, the helper at line 6) gave "deserialize error: unsupported value type `function`", which doesn't say which argument or what to pass instead.
- Lua patterns cost about 30 requests. The model checked text with `test.truthy(s:find("..."))` instead of `test.contains`, which matches plain text. `string.find` treats `-` as a quantifier, so `find("go run ./cmd/cog --docs > docs/words.md")` failed (steps 39 to 53: it blamed `>` first, ran nine `lua5.4 -e` experiments, then found `-`) and later `find("fix-parse")` failed the same way (steps 118 to 136: it blamed the UTF-8 bytes of the ✓ and ✗, wrote the wrong byte escapes, and got it working only because it had switched to plain find along the way).

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`.

Possible fixes:
- Add `test.neq`, `test.falsy`, `test.not_contains`, `test.matches` (a pattern) and `test.errors(fn, text?)` (expects a Lua error), with messages like the existing ones.
- List every `test.ui.answer` kind (confirm, select, input, custom, exec, http) and the shape each takes.
- Say in the test docs that `test.contains` matches plain text, and that `string.find` patterns treat `- . % ( ) [ ] * + ? ^ $` specially unless given `1, true`.
- Name the argument in the conversion error: "test.ui.answer: the value can't be a function; pass a table like { code = 0, stdout = \"...\" }".

**Fixed 2026-11-03:** `test.neq`, `test.falsy`, `test.not_contains`, `test.matches` (a Lua pattern) and `test.errors(fn, text?)`; `test.ui.answer` and `test.mock_tool` name themselves and the shape to pass when given a function; every answer kind is documented in `docs/extensions.md`, and `psi help extensions` now lists the test helpers and assertions, with the note on Lua patterns in the guide.

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

**Fixed 2026-11-03:** inside tmux with `extended-keys` off, startup says so once with the `~/.tmux.conf` lines and that Ctrl+J works; `psi help commands` names Ctrl+J next to Shift+Enter (`/help` already listed it).

### `/tmp` doesn't last between bash calls (added 2026-11-03, from the session review)

The model built `/tmp/cog_bin` and wrote `/tmp/w.md` in one bash call and looked for them in the next (steps 307 to 327); they were gone, since bubblewrap mounts a private `/tmp` per call (`crates/psi-safety/src/isolation.rs`, `--tmpfs /tmp`). It concluded "each bash call is a fresh process. The binary isn't persisting", spent about eight requests on it and rebuilt the binary in every later call. The prompt says to keep files in `$TMPDIR`, but models write `/tmp` by habit; on macOS (Seatbelt) `/tmp` is shared, so the same commands work there.

Possible fixes:
- Bind the session's `$TMPDIR` at `/tmp` too (`--bind <tmpdir> /tmp` in place of `--tmpfs /tmp`): `/tmp` lasts for the session and still isn't the host's, and Linux behaves like macOS.
- Or, when a bash command names `/tmp/`, add to the result: "/tmp is private to each call and is gone now; keep files for later calls in $TMPDIR."

**Fixed 2026-11-03:** bubblewrap binds the session's `$TMPDIR` at `/tmp` (plan §8.4 updated), so `/tmp` lasts for the session; the prompt line about a private `/tmp` is gone.

### Scout refused `cd` (added 2026-11-03, from the session review)

Scout's first three calls were `cd /home/ndari/projects/cog && go build ...` and were refused with "cd isn't on the read-only list"; `cd` isn't in `READ_ONLY` (`crates/psi-safety/src/read_only.rs`), though it changes nothing but the shell's own directory. The refusal for `go` that followed was right (scout doesn't build, see the scout entry above), and so was `export PATH=...`; the model then tried `which go`, `/usr/local/go/bin/go` and `find / -name go`, six requests before it gave up on running anything.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.agents/01-scout.*`, steps 1 to 6.

Fix: add `cd` (and `pushd`, `popd`) to the read-only list, with a test.

**Fixed 2026-11-03:** `cd`, `pushd` and `popd` are on the read-only list, with tests.

### The todo tool's `toggle` confused the model (added 2026-11-03, from the session review)

Asked to fix the high and medium items, the model first toggled all six of them (steps 203 to 208), marking them done before any work. At the end it toggled each again to mark it done, found it now "not done", and toggled it back (steps 244 to 253), until the no-progress guard fired. `toggle` flips the state (`crates/psi-tools/src/todo.rs`), so calling it to mean "done" works only when the model remembers the current state. The `done` action already exists and always marks done, but the description only names `toggle`.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`.

Possible fixes:
- Name `done` (id) and `undo` (id) in the description and drop `toggle` from it (still accepted). Both are safe to repeat, so a repeated call changes nothing.
- Maybe a third state, `start` (in progress), since marking work as started is probably what the first six toggles meant.

**Fixed 2026-11-03:** the description names `done` and `undo`; both set the state and say "was already done" when nothing changes, and `toggle` now means done.

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

**Fixed 2026-11-03 (plan §11.2):** a single newline is a line break; headings drop their `#`s and are bold in the heading color (yellow by default; level 1 also underlined); an all-bold line is a title in the heading color; pi's other markdown tokens are themable, with defaults close to pi's dark theme; quotes have a `│` bar. `just md-preview <file>` shows a file as Psi draws it, and snapshot tests of three answers from this session plus an elements file keep the look (`just md-bless` after a deliberate change). Left for later: clickable `path:line`.
