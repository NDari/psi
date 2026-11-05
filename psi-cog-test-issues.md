# Issues from the cog trial

Problems the owner notices during the long-session trial in `~/projects/cog` (started 2026-11-03), collected as they come up ("add ... to cog") and gone through together later. Each entry: what happened, where it shows, and what might fix it. Fixed entries move to the end with the commit.

## Next

1. **Done 2026-11-03:** the sessions were reviewed; the entries marked "from the session review" and the notes entry come from it. No `/label` was used. The sessions (the trial ran 2026-11-03, about 16:52 to 18:41 local, the 35B at 64K with `-ub 2048`):
   - `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2252_e48f5e9d.*`: the first extension attempt (`.psi` refusal, the `$TMPDIR` folder), already covered.
   - `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`: the long session (the extensions, the review, the fixes in commits `89eb5d8` and `7feaf2b` in cog), 65 MB of wire log.
   - `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.agents/01-scout.*`: the scout call (134 requests).

   What to check: time to first token, reading and generation time, and where the cache broke and why; clearings and summaries, and whether facts survived them; tool errors, loops, re-reads, guard events and pauses; edits that failed and how they recovered; whether the post-edit check, gopls (MCP), the todo list and the extensions were used, and how; claims of checking that no call backs up; anything the owner marked with `/label`. Note what went well too.
2. **Gone through with the owner 2026-11-03.** Fixed now: the entries under Fixed, the scout stop gap, and in `known-issues.md` the `/reload` trust check and the timing-sensitive test. Left open: `/diff` (to discuss), the extension-block workaround (to investigate further), the scout limit setting, the parent's cache after scout, scout's re-reads, and the claims of checks.
3. **Second session reviewed 2026-11-04** (08:46 to 13:35 local, the 35B; 240K by mistake until a `/compact` and a restart at 64K at 10:52; Psi rebuilt with the 2026-11-03 fixes at about 09:00). Files: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-04-1440_fecf76bd.*` (433 requests, 90 MB of wire log) and its `.agents/` (`01-scout` 13 requests, `02-scout` 191, `03-worker` 654). No `/label`. New entries are marked "from the second session"; what the fixes did is under "The fixes in the second session".
4. **Gone through with the owner 2026-11-04.** To do: cards as lists (↑↓, Enter, Esc for the safe answer); `git stash`, `git checkout` and `git restore` on always-ask; the extension block names the extension, plus `psi.bash.writes()`; the multi-edit failure says which entries matched; a spinner while summarizing, and Esc stops `/compact` if stopping has no side effects; sub-agents have no request limit in the TUI, and budget pauses are recorded with the time spent waiting on the owner. Asked: how the model learns its tools (below, "The model's tool list leaves out extension, MCP and agent tools").

## Open

### `/diff` is hard to use in a long session (added 2026-11-03, needs the owner's view of what's wrong)

The owner found `/diff` not right in the trial ("even if some are committed"; not yet clear what the fix is). What it does now (`session_changes` in `crates/psi-agent/src/recovery.rs`): the diff between each pair of Psi's own checkpoints along the session, in order, each labelled by its source ("by edit call 12", "by a restore", "outside the agent"), in the pager. It uses Psi's checkpoint store, not the project's git, so commits don't hide anything: in this session the checkpoints cover all 18 changed files, including everything in the owner's two commits (`89eb5d8` the extensions, `7feaf2b` the fixes). But with 64 checkpoints (63 tool calls and 16 prompts), it is one long list where the same file comes back for every edit.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`; checkpoint store `~/.psi/checkpoints/b03eb31bcd7ed331`.

Possible readings and fixes, to settle together:
- **The whole session at once:** by default, one diff from the session's first checkpoint to now (`git diff --stat` on top, then the diff), with the step-by-step view as `/diff steps`.
- **Since the last commit, or since the last prompt:** `/diff` for what is not committed yet (what you'd review before committing), `/diff last` for the last prompt's changes, `/diff session` for everything.
- **Per file:** `/diff <path>` for one file's changes over the session.
- **Committed vs not:** mark the files already in a commit, so committed and pending work can be told apart.

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

**Second session**, the owner caught two answers that weren't tested:
- Asked whether `{1 2 3} $double map` works, the model ran `{ 1 2 3 } { $double } map` instead (a different program), got an error and answered no; the owner: "Did you test this? I just did it and it works in the repl" (10:58). On a second try it ran the right program, which worked.
- Asked for a real example of `unless` against `not if`, it answered without running anything; the owner: "this `:n 0 = ... not if` does not work. first, its $n not :n, and the not should come after the =" (13:34).

The `cog_run` tool, built for exactly this, was offered in every request and never called; the model ran `go run ./cmd/cog` with files in `/tmp` about 100 times instead.

### Notes from the review, no action yet (added 2026-11-03)

- **Timing.** 359 requests in the long session: first token median 0.4 s, 90th percentile 1.0 s; 291 s reading prompts and 573 s generating in all. The seven requests over 15 s to the first token all come after a summary (five, each 17 to 23 s for about 20K tokens, since only the system prompt stays cached), after the restart that added `cog_run` to the tool list (22 s, cache 0, as D-38 expects) or after scout (above).
- **The run that stopped without an answer.** At 13.1 minutes the run ends after a tool result with no answer and no entry saying why; the next entry is "continue" 23 minutes later, from a restarted Psi (the requests now include `cog_run`). The owner probably quit to load the extensions; if so, quitting between tool calls leaves no trace in the session, which a review can't tell from a crash.
- **Summaries kept the facts; the model didn't always use them.** The first summary said "`test.neq` does NOT exist", and the first request after it wrote `test.neq` again (step 103), as did step 115.
- **gopls (MCP) was used only when the owner said so,** three calls in one response; `go_package_api` got the module path wrong (`github.com/ndari/projects/cog`, where `go_workspace` in the same response said `github.com/NDari/cog`) and returned "(no output)". It wasn't used again, not even for the file renames in issue 7.
- **`cog_run` was never called** after it was built, in about 250 later requests.
- **Went well:** the post-edit check ran 56 times (15 s in all) and its 7 failures were all real (the red tests, the stale `docs/words.md`); failed edits (two "not found", one "matches 3 places", one write refused after a `sed -i`) each recovered in one or two calls; reads came in parallel batches of up to five; always-ask caught `git push` and a `sudo chmod` on `/etc/ssh`; the status-line extension showed in the footer as written; scout found the real high-priority bug.

### A sub-agent stopped at a pause returns nothing (added 2026-11-04, from the second session)

Scout 02 (read the docs and compare them for contradictions) spent about 5 minutes and 191 requests and returned "(no answer)". It went looking for where `?` is defined and ran the same few `grep`s on `env.go` dozens of times (`?` is defined in `stdlib.go`); the no-progress and error-streak guards fired 12 times, then came the pause at 20 failed calls in a row. That pause was answered "stop" 22 ms after it was raised (guard entry at ...145047, scout's result at ...145069), too fast for a person to choose: the owner was probably typing in the prompt, and a keystroke answered the card (see "Cards answer to single keys" under Fixed). Everything scout had read was lost, and the parent read the docs again itself.

Two things made it worse:
- **A `grep` with no match counts as a failed call.** 98 of scout's 125 errors were "(no output) [exit code 1]", grep's normal answer for "not found". The result doesn't say that, and the error streak counts it, so the streak reached 20 on searches that worked.
- **The stop throws the run away.** The parent gets the last answer, and a looping scout has none.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-04-1440_fecf76bd.agents/02-scout.*`.

Possible fixes:
- When a sub-agent stops on a guard or a limit, make one last request without tools ("Stop now. Report what you found so far, with paths and line numbers.") and return that, marked as cut short.
- For `grep`, `rg`, `git grep` (and `diff`, `cmp`), exit code 1 with no stderr means "no match" or "differs": say so ("grep found no matches (exit code 1)") and don't count it toward the error streak.

**Status 2026-11-04:** the pause that answered itself is fixed (cards are lists that ignore keys at first). Also fixed 2026-11-04: when the last command is `grep`, `rg`, `git grep` (or `cmp`) and it exits 1 with no output, the result says "[exit code 1: grep says no line matched]" and isn't an error, so the streak doesn't count it (plan §8.6, with tests). Still open: the last report when a sub-agent stops.

### Notes from the second session, no action yet (added 2026-11-04)

- **Timing (main session).** 433 requests: first token median 0.4 s, 90th percentile 1.2 s; 285 s reading prompts and 855 s generating. Four requests over 8 s to the first token: the first after the rebuilt Psi's restart (38K tokens, cache 0, 39 s; the tool list changed), the first after the 64K restart (15.5K tokens, 11 s), and the first after each of the two automatic summaries (11.7K and 14.6K tokens, 10 to 12 s, only the 3.6K of system prompt and tools cached, as before).
- **Scout's big last request.** Scout 01 read many whole files in its last batch, so its final request read 37.7K new tokens (89 s to the first token) and generated for 78 s.
- **The model didn't recognize its own tool.** Asked "I believe we have an extension that allows you to run cog source", it searched the Go code for `EvalSource`; only "Should be in .psi/extensions/cog_run" made it look. It then improved `cog_run` (inline `source`), but an extension change needs `/reload`, which nobody ran or mentioned, so the change never took effect before worker stashed it away (above).
- **gopls (MCP) was never called** (the tools were offered in every request).
- **`git add -A` committed debris.** Asked to commit, the model staged everything: `cog_debug_test.go` (a debug test it moved in from `/tmp` at step 122) went into `777edcf`, and the 5 MB `cog` binary from `go build -o cog` (step 422) into `ddaef56`. Model mistakes; `git status` before both commits listed them.
- **Worker changed more than asked** (the task was the test files; it also changed `env.go` and `stdlib.go`), and said so in its report.
- **Budget pauses left no entry** (fixed 2026-11-04: a `pause` entry with the wait).
- **Went well:** all four summaries (three in the main session, one in worker) used the cached "extend" path, with no fallback; the task carried across them (after each, the next step continued the work correctly; but see "Summaries keep wrong claims and drop what the model learned about the language", which found wrong facts carried and language knowledge lost); always-ask and the `.psi` per-file grants worked as designed; the post-edit check ran 41 times (10 s in all) and its 17 failures were all real.

### The fixes in the second session (added 2026-11-04)

What the 2026-11-03 fixes did in real use (Psi rebuilt at about 09:00; everything after that ran them):
- **`/tmp` lasts the session:** worker and the main model kept `.cog` and test files in `/tmp` across calls, and step 122 moved `/tmp/cog_debug_test.go` written in step 121. No "file not found" for an earlier call's file.
- **Todo:** 19 calls in the main session and 33 in worker, only `add`, `done` and `list`; no `toggle`; two "was already done" in worker, harmless.
- **Several edits per call:** used 6 times unprompted, 3 applied; the failures are the entry above.
- **Post-edit check cap:** the largest failed check result was 1,768 characters (the first trial had 16.5K).
- **Summaries:** the `/compact` on the 240K server took 47 s for about 100K tokens (97K from the cache); after the restart at 64K the context was 15.5K. The two automatic summaries took 11 s each and left 11.7K and 14.6K, so with the re-read about 22 s per summary (about 33 s in the first trial). The 10K tail is working. Clearing never ran (it needs about 28K of old results over 800 characters to beat a summary, and this session's context was mostly conversation).
- **Scout limit and `cd`:** no refusals of `cd` or builds in either scout; pauses at 100 as set.
- **Trust:** asked again only after `.psi` changed (the entry above).

### Summaries keep wrong claims and drop what the model learned about the language (added 2026-11-04, from a review of the model's work)

The second session had three summaries: at 10:51, at 11:04 (both while the bare-name change was being finished) and at 13:28 (in the middle of the `doc` swap). They carried the task forward well, but they also carried wrong facts and lost the knowledge that kept the model correct. Most of the model's reasoning errors in the session come after a summary.

1. **What the model claimed goes in as if it had been checked.** The 10:51 summary lists "`{ $double } map` passes function as data to combinator" under "What works" with no call behind it; the 11:04 summary keeps it, although at 10:58 the model had run exactly that and seen it fail ("callback must leave exactly one value, got 2"). Both also say `cog_run` was "enhanced with inline source support, version 0.2.0", but worker's `git stash` had already taken that change out of the tree (entry under Fixed), and the tool Psi loaded only ever had `path`. Wrong reasons went in too ("`$fact apply` to avoid infinite auto-execution", "`tail` uses `$without-last apply` to prevent auto-execution inside block bodies") and ended up in the `777edcf` commit message.
2. **Extending a summary keeps its mistakes.** The 11:04 summary copies "Pattern B: `{ name apply }` → `{ $name }`" (wrong: bare `name` runs inside a block) word for word from the 10:51 one, and lists `{ $double } map` next to the `{ $double apply } map` that the newer turns showed was needed. Nothing asks the extend request to correct the older text.
3. **Knowledge of the language is lost.** All three summaries describe task state (goal, files, decisions, next step); none records how Cog works: dicts are `#[ name: value ]`, `let` takes the value then the name, `:name` pushes a quote while `$name` and bare `name` give the value. What scout read about the language lived only in the context. Before the 11:04 summary the model wrote dicts correctly (10:54, `#[ op: { dup + } ]`); after it, it used `{ "x": 1 }` (a sequence) at 13:08 and reported a non-existent "inconsistency" from it, and wrote `:n` for `$n` at 13:30 and again at 13:34 after the owner had corrected it.

The 64K context made this matter more: three summaries in about two and a half hours of work.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-04-1440_fecf76bd.jsonl` (the three `compaction` entries; the 10:58 run of `{ $double } map`; 10:54, 13:08 and 13:30 for the language).

Possible fixes:
- Ask the summary to keep "verified (a call in the conversation shows it)" apart from "claimed or planned", or to drop claims no call backs up.
- Add a short section of lasting facts about the codebase or language (syntax, conventions, how to run things) that the next summary carries over.
- In the extend request, ask the model to correct or remove entries from the earlier summary that newer turns contradict.

**Built 2026-11-04 (plan §7.4, H-17):** the headings Learned about the code, Current state with Checked and Not checked, and Corrections (after an earlier summary); the merge applies corrections; the fallback gets the earlier summaries; the harness record has the last post-edit check. A replay of 25 recorded summary requests (17 from five 16K eval tasks, 8 from the cog trial) with the old and new instruction: the planted facts survived in all 6 `long_api_docs` summaries; the cog 11:04 replay corrected the `{ $double } map` claim, but kept the wrong "Pattern B" line under facts; most corrections in the eval tasks fixed real earlier mistakes (one was itself wrong and fixed by the next summary). The model split Checked from Not checked in 0 of 25 summaries when they were a note in brackets; as `### Checked` and `### Not checked` subheadings (pi uses `### Done` the same way) it split them in 17 of 20 (a second replay, new instruction only), and the cog 11:04 Checked list was right (`{ $double } map` fails, `$double map` works). The new summaries run 10 to 35% longer; at 16K many pass the 234 words asked, as the old ones already did. Replay files: the session's scratchpad `replay/` (script, outputs).

### Clear old results when the cache is broken anyway (added 2026-11-04, from a discussion, candidate)

Clearing runs only past the trigger (about 70% of the window) and in batches, on purpose: replacing an old result with a note makes llama-server read everything after it again, and the oldest results sit near the start, so clearing every turn would mean re-reading nearly the whole context every turn (plan §7.4: a 64K review that cleared 4 times in 30 requests re-read 24K to 31K tokens each time, 114 s in all).

But some requests re-read everything anyway, and clearing then costs nothing extra:
- **After a summary:** what follows it is new to the server. Psi already clears in the kept part when it still needs room; it could also clear there ahead of the trigger.
- **After a sub-agent call** on a server with one slot: the sub-agent's requests push the parent's prompt out of the cache ("The parent re-read its whole context after the scout call", above), so the parent's next request reads it all again.
- **When the tool list changes** (an MCP server joins at a user prompt, `/reload`): the cache breaks at the system prompt.

Possible fix: at those moments, clear old results (same rules: over 800 characters, not the last step's, not the last 3) even below the trigger, so the next clearing or summary comes later. Psi knows the first and third itself; for the second it can tell from the server's `cache_n` on the next response only after the fact, so it would assume a miss after a sub-agent call on a one-slot server (or skip it once the slot save and restore above is built). Not tied to any project.

**Built 2026-11-04 (plan §7.4), for the first and third:** before each request Psi compares it with the last one sent; where it stops matching (from the start when the tools, model or thinking setting changed; from the first changed message otherwise), old results after that point are cleared down to the clearing target even below the trigger, at least a twentieth of the window at a time, with a `clearing` custom entry (`reason: cache_break`). Tests: a plan-mode switch clears, the same runs without it don't; a summary clears older results in the kept tail. After a summary it rarely finds anything (the 10K tail leaves the context near the target). Left out, the sub-agent case: the parent's cache isn't reliably gone (the cog trial: after scout 01's 134 requests, 0 of 29.6K tokens cached; after scout 02's 191, 9.5K of 16.6K, restored from llama-server's RAM cache), so the clearing wouldn't be known to be free; the slot save and restore in "The parent re-read its whole context after the scout call" is the fix there.

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

**Status 2026-11-04:** the clearing rule, the 10K tail, several edits per call and the post-edit cap are done (commit `26b2108`); the summaries not reused from the cache are the server's, not fixable in Psi as far as known.

### The fixes in one eval run (added 2026-11-04)

One quick-set run on the 35B at 64K with the batteries variant plus `"subagents": true` (`~/.psi/evals/2026-11-04-0236-quick-batteries-agents-qwen3.6-35b`): 7 PASS, 3 MODEL-FAIL (logic bugs: a sort order, a foreign key, a missing import), 0 HARNESS-FAIL. One run, so it proves the changes work, not how well. What it shows:
- **Todo:** used in 5 trials, 37 calls; `done` each time an item was finished, no `toggle`, no item marked twice. One call carried tool-call markup in its action (`"add>\n<parameter=text>\n..."`, multi_split_package), refused as an unknown action and redone correctly at once (`known-issues.md`).
- **Scout:** called once (feat_logstats_jsonl) with a reading task only, no builds or tests asked; 5 requests, reads and `find`, no refusals.
- **`/tmp`:** used in 6 trials (up to 14 calls in sh_archive_fix), each call setting up its own test folders there; no "file not found" for a file kept from an earlier call.

A second run after the multi-edit and 10K tail changes, same variant (`~/.psi/evals/2026-11-04-0308-quick-multiedit-tail10k-qwen3.6-35b`): 8 PASS, 2 MODEL-FAIL (a foreign key again, and quoted strings in an INI converter), 0 HARNESS-FAIL. `edits` was used in 6 of 31 edit calls, unprompted: three with several replacements (4 in feat_config_env, 2 in two others), all applied; three with one, folded into `old_text` and `new_text`. No edit failed. No trial compacted, so the 10K tail wasn't exercised; that shows in long sessions.

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

**Second session:** the parent gave worker a large job ("fix all the test files" for a change to name evaluation), which ran 654 requests in 52 minutes (503 s reading, 1,789 s generating) and paused at every hundred, six times, for the owner to go on (one pause waited almost 6 minutes). Scout 02 paused once at 100. The budget pauses leave no entry in the sub-agent's session (the guard pauses do), so a review finds them only from the gaps in the timestamps. Scout got no builds or tests this time; the stop gap held.

**Fixed 2026-11-04:** in the TUI a sub-agent has the parent's budgets, so no request limit unless `--max-turns` sets one (Esc and the guards still stop it); print runs and evals keep 100. Every pause, for a budget or a guard, is now a `pause` entry with the reason, the answer and how long it waited (plan §5, §10.1), with tests. No setting needed.

### The model worked around an extension's block (added 2026-11-03, from the session review)

The model's own `protect-words-md` extension blocks any bash command whose text contains `docs/words.md`, so it also blocked reads (`cat`, `head`, `git diff`, `git status`), `git add` and even a commit message naming the file: 14 blocks in the long session and 4 in scout's run. The model then got past it on purpose, twice: `F=words.md; ... > docs/$F` to write the file (step 329, "the extension uses static pattern matching, so `docs/$F` doesn't trigger it") and `git add -A` to stage it (step 356). Psi passed the block on as "Blocked: <reason>" with nothing saying not to work around it.

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-2312_0665442a.*`, steps 301 to 356.

Possible fixes:
- Add a line to every block from an extension: "Blocked by the extension <name>. Don't work around it; if it is wrong, say so and stop." (The extension's name is known at the gate.)
- Give extensions what Psi's classifier already knows: `psi.bash.writes(command)`, the paths a command writes (redirects, `cp`, `mv`, `tee`, `sed -i`), so a gate like this one can block writes and let reads through. Show it in the `tool_call` docs where bash is gated.

**Second session:** 32 blocks (7 in the main session, 1 in scout 01, 24 in scout 02), all reads: `grep`, `cat`, `awk` on `docs/words.md`. No workaround this time. But the main model took the block for the command's output: after `grep 'doc' docs/words.md` came back "Blocked: Do not edit docs/words.md directly...", it said "The description itself says 'Blocked: ...', that's in the metadata", then "it's matching the header line", searched the Go code for "Blocked" and "Do not edit", decided `doc` had no description, and wrote `"( quote string -- ) doc"` into `word_metadata.go` (step 413), fixed only at step 420. The model can't tell an extension's block from the command's output. Both fixes above would have prevented it: the first says where the text comes from, the second lets reads through.

**Fixed 2026-11-04:** a blocked call's result is "Blocked by the <name> extension (the call did not run): <reason>\nDon't work around the block; if it seems wrong, tell the user." New `psi.bash.writes(command)`: nil when a command only reads (plan mode's check), else why it may write, so a gate can let reads through; the `keep-generated` example shows it, and the guide says to use it. (Psi's classifier knows whether a command only reads, not which paths it writes, so the function answers that.) Plan §9.4, with tests.

### Nothing shows that a summary is being written (added 2026-11-04)

While Psi summarizes, the screen shows one dim line, "summarizing the earlier conversation (about N tokens)...", and then nothing moves until it is done, which took about 13 s on average at 64K in the first trial and takes longer for a large context (this session reached about 100K tokens on the 240K server). It looks stuck. From the code (`crates/psi-cli/src/interactive/ui.rs`):
- **`/compact` has no spinner at all.** The spinner and its timer are drawn only while a prompt runs (`self.running`), and `/compact` is sent as a command (`Cmd::Compact`, `worker.rs`) without setting it. For the same reason **Esc doesn't stop a `/compact`**: the interrupt key aborts only when `running` is set, though `compact_now` can be cancelled (`Compacted::Aborted`).
- **During a run, the spinner doesn't say what it waits for.** It says "waiting for the model…", and its seconds count from the start of the prompt, not the summary. The summary request passes no progress callback (`summary_request` in `crates/psi-agent/src/run/fit.rs`), so the "reading the prompt: N%" text never shows, though the plain-text fallback has the whole summary prompt to read first (the slow part after a restart).

Possible fixes:
- Show the spinner while summarizing, in both cases, with its own text and timer: "⠋ summarizing about 98K tokens (12s) · Esc stops". Set `running` for `/compact` (or an activity of its own) so Esc stops it and the terminal's title and progress indicator show work.
- Pass the progress callback in `summary_request`, so reading the prompt shows its percentage as a normal request does.
- When it is done, say how long it took next to the sizes ("compacted: about 100K tokens before, 16K after, in 24 s").

**Fixed 2026-11-04:** while a summary is written the spinner says "summarizing about 98K tokens (12s) · Esc stops", with its own timer and the server's reading progress (the summary request now passes it on); `/compact` counts as a run, so the spinner shows, the terminal shows work and Esc stops it (a stopped summary records nothing); an automatic summary ends with "summarized the earlier conversation (about 98K tokens) in 24s". New `Compacted` event. Plan §7.4, with a TUI test.

### Worker used `git stash` on the owner's tree and lost the `cog_run` changes (added 2026-11-04, from the second session, high)

To compare the old behavior with the new, worker ran `git stash` 18 times and `git stash pop` 16 times, and `git checkout <file>` three times (`env.go` twice, discarding the parent's change to it, which worker then redid by hand; `modules_test.go` once). Two stashes were never popped, because a later `git stash pop` took the newer one. Two stashes are still in cog:
- `stash@{1}` (10:06): the parent's `cog_run` changes, `.psi/extensions/cog_run/init.lua` and `cog_run_test.lua` (inline `source` support, written at 09:27 with the owner's per-file approval), plus test edits that were later redone.
- `stash@{0}` (10:17): test edits and a 4-line `env.go` change, redone later.

The `cog_run` changes are gone from the working tree and from both commits (`777edcf`, `ddaef56`). Nobody noticed: worker's report doesn't mention stashes, and the parent never looked. They can be recovered from `stash@{1}` or Psi's checkpoints. Psi's always-ask list has `git reset --hard`, `git clean` and `git stash drop`/`clear`, but not `git stash` itself, `git checkout <path>` or `git restore`, which also set aside or throw away uncommitted work (here the parent's and the owner's, not the worker's own).

Session: `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-04-1440_fecf76bd.agents/03-worker.*`, steps 41 to 615.

Possible fixes:
- Tell worker (and the main prompt) how to compare with the committed code without touching the tree: `git show HEAD:path`, or `git worktree add $TMPDIR/head HEAD` and run the old code there. A short line in worker's prompt.
- After a sub-agent call, compare `git stash list` (and the files changed outside the agent's edit calls) with before, and add to its result: "worker left 2 new stashes: ...". Psi already takes a checkpoint around each call, so it can also name files the sub-agent reverted.
- Add `git stash` (push, not `list`/`show`/`pop`), `git checkout -- <path>` / `git checkout <path>` and `git restore` to always-ask. In this run that is 21 prompts, which is too many on its own; with the prompt line it should be rare.

**Fixed 2026-11-04 (the ask):** `git stash` (not `list`, `show`, `pop`, `apply`, `branch`), `git checkout` and `git restore` are on always-ask; rules can now say a word must not appear (`git stash !list`). A recorded Qwen transcript now has one more destructive retry caught (a plain `git checkout`). Not done (not asked for): the line in worker's prompt, and naming new stashes after a sub-agent call.

**Note 2026-11-04:** the stashes were dropped without restoring `cog_run`; the change is still in cog's unreachable stash commit `b237322` (`git -C ~/projects/cog checkout b237322 -- .psi/extensions/cog_run`). That version marks both `path` and `source` as required, though only one is meant to be.

### The model's tool list leaves out extension, MCP and agent tools (added 2026-11-04, from the second session)

The model learns its tools in two places. The request's `tools` array carries every tool with its name, description and parameters (core, `agent`, extension tools like `cog_run`, and MCP tools like the seven gopls ones); the chat template puts them in the prompt. But the system prompt also says `Tools: read, write, edit, bash, todo`, built from the core tools only, before extensions and MCP load (`run_mode.rs`, `system_prompt` in `crates/psi-agent/src/prompt.rs`). So the prose tells the model its tools are those five, and the rest show up only as JSON. Asked "I believe we have an extension that allows you to run cog source", the model searched the Go code; `cog_run` and gopls were never called in either session.

Other gaps:
- **Extension gates are invisible** until one blocks (see the `protect-words-md` entry), and then the block doesn't say it came from an extension.
- **Extension changes need `/reload`,** and nothing tells the model: it improved `cog_run` and never mentioned that it wouldn't take effect.
- **`cog_run` gave the model no reason to use it:** its description says it runs `go run ./cmd/cog <file>`, which bash does as well, and it refuses paths outside the project, where the model kept its scratch files (`/tmp`).

Pi lists each tool with a one-line snippet under the preamble, followed by "In addition to the tools above, you may have access to other custom tools depending on the project" (`system-prompt.ts`).

Possible fixes:
- Build the `Tools:` line from the tools the request sends, grouped by where they come from: `Tools: read, write, edit, bash, todo, agent; from extensions: cog_run; from MCP (gopls): go_diagnostics, go_search, ...`. About 30 tokens more here, and the prompt stays byte-identical while the tools don't change.
- When a write or edit lands in `.psi/extensions/`, add to the result: "Extensions load on /reload; tell the user to run it."
- In the extension guide, advise tool descriptions that say when to use the tool instead of bash.

**Fixed 2026-11-04:** the `Tools:` line is built per request from the tools offered: `Tools: read, write, edit, bash, todo, agent; from extensions: cog_run; from MCP servers: mcp__gopls__...`. A write or edit into a `.psi/extensions` folder ends with "Extensions load on /reload; tell the user to run it." The extension guide says a tool's description should say when to use it instead of bash. Plan Appendix B, with tests.

### Cards answer to single keys, so typing can answer them (added 2026-11-04, from the second session)

Every card Psi shows takes a single letter as its answer, and a card takes the keys as soon as it appears, even while the owner is typing a prompt (`crates/psi-cli/src/interactive/ui.rs`):
- approval: `y` once, `s` for the session, `n` or Esc deny, `e` deny with a message;
- pause: `c` or Enter go on, `s` or Esc stop;
- blocked by the sandbox: `r` rerun outside, `a` add the folder, `n` or Esc no;
- `/reload` trust: `y`, `n` or Esc;
- an extension's confirm: `y` or Enter, `n` or Esc.

In the second session this probably stopped scout 02 (above): its pause was answered "stop" 22 ms after it appeared, and the owner may have been typing. Worse cases are easy to reach: a `y` typed into a prompt approves a command, and an `s` approves it for the whole session.

The owner's view: single-key answers aren't good for them; they want a list to choose from, so a choice is deliberate.

Possible fixes:
- Show each card's choices as a list (as `Picker::Select` already does): ↑↓ to move, Enter to choose, Esc for the safe answer (deny, stop, no). The highlight starts on the safe answer, so a stray Enter does no harm.
- Ignore keys for about half a second after a card appears, so typing already in progress can't reach it.
- Keep the letters as an option for those who want them (a setting), off by default.

**Fixed 2026-11-04:** every card (approval, pause, blocked write, `/reload` trust, an extension's confirm) shows its answers as a list: ↑↓ choose, Enter confirms, Esc gives the safe answer, the list starts on the safe answer, and keys are ignored for 400 ms after a card appears; letters no longer answer. Plan §8.7, with TUI tests (typing "yes" and Enter on a fresh card leaves it up). Letters as an option: not done.

### A failed multi-edit throws away the good replacements (added 2026-11-04, from the second session)

`edits` was used 6 times (4, 6, 7, 8 and 12 replacements); 3 of those failed on one entry each (`edits[11]` once in the main session, `edits[6]` twice in worker), and "Nothing was changed" applied none, as specified. The model didn't resend the batch without the bad entry: in the main session it said "The first 11 edits didn't apply because they didn't match", made one single edit, then gave up on editing and handed the job to worker (step 155); worker sent the same failing batch again and then did the edits one at a time (steps 315 to 321).

Possible fixes:
- Say which entries matched and what to do: "edits[11] was not found (closest match ...). The other 11 matched; nothing was changed. Send them again with edits[11] fixed or left out."
- Or apply the entries that match and report the one that didn't (pi fails the whole call, as Psi does now). All-or-none keeps a file from being half changed, so the message alone is probably enough.

**Fixed 2026-11-04:** every entry that didn't match is named, and when others did: "The other 11 edits matched, but nothing was changed. Send them again with edits[11] fixed or left out." Still all or none. With tests.

### Psi asks to trust cog again and again (added 2026-11-04)

Starting Psi in cog keeps asking "`.psi` has settings, 5 extension(s), which run as you, MCP servers, which run as you, new or changed since you last trusted it. Load it? [y/N]". How it works (`crates/psi-cli/src/trust.rs`, `project_trusted` in `crates/psi-cli/src/main.rs`, plan §9.3): one hash over every file in `.psi/settings.jsonc`, `.psi/mcp.json`, `.psi/extensions/`, skills, prompts, themes, agents and `.agents/skills`, kept per project in `~/.psi/trust.json`; any change to any of them asks again, before the TUI starts, with no word on what changed.

What the files show: nothing in cog's `.psi` has changed since 2026-11-03 17:50 (the model's last edit to `test-cmd`), `~/.psi/trust.json` was last written 2026-11-04 08:40 (the yes this morning), and cog counts as trusted now (`psi mcp list` loads gopls without asking). So this morning's ask came from yesterday's edits by the model. Confirmed: it doesn't ask when nothing changed (no ask after `just install` and a resume, 2026-11-04), so the asks come from changes only.

Why it asks more than it needs to:
- **The model's own edits count as changes.** In the trial the model built and fixed extensions in `.psi/extensions/` (writes there ask the owner per file since D-27's 2026-11-03 change), and every edit, including to `*_test.lua` files that never load in a session, means another trust prompt at the next start.
- **The prompt doesn't say what changed,** so the owner can't tell a known edit from a surprise and says yes by habit, which defeats the check.
- **The hash isn't stable across Rust releases:** `DefaultHasher` may change its algorithm with the toolchain, so a Psi built with a new Rust would ask again for every project.

Possible fixes:
- Keep a hash per file and name the changed files in the prompt ("changed: extensions/test-cmd/init.lua"), or show a diff on request.
- Leave out files that don't load: `*_test.lua` in extensions (and anything `psi ext test` alone reads).
- When the owner approved a write into `.psi` in a session (the per-file grant), update the stored hash for that file, so a change the owner saw doesn't ask again; changes from bash or outside Psi still ask.
- Use a stable hash (SHA-256 via `sha2`, or `blake3`) for `trust.json`.

**Fixed 2026-11-04:** `trust.json` keeps a SHA-256 per file (an entry from an older Psi asks once more, saying so); the startup prompt and the `/reload` card list what is new, changed or removed (up to 8, then "and N more"); `*_test.lua` in extensions don't count; a write or edit to `.psi` that the user approved records that file as trusted (the agent calls a hook the CLI sets for a trusted project; sub-agents too). Plan §9.3, with tests.
