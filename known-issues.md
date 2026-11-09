# Known issues

Problems we know about and have chosen not to fix yet, with how much they matter. When one is fixed, remove it here and say so in the commit.

## Open

### Bash can write what the file tools guard (added 2026-11-03; the first item's high-priority part, `/reload`, fixed 2026-11-04)

The file tools ask before changing Psi's configuration or files checkpoints can't undo, but bash only goes through the sandbox and the always-ask list. A heredoc is the easy way (the cog trial on 2026-11-03 wrote an extension with `cat > .psi/extensions/cog_run/init.lua << 'EOF'` after `write` was refused), but `cp`, `sed` or a script do the same.
- **The project's `.psi/` through `/reload` (high).** The sandbox lets bash write the whole workspace, `.psi/` included, so a model can change settings and extensions without asking, and an extension's Lua runs inside Psi, outside the sandbox. At startup this is covered: trust is a fingerprint of `.psi/`'s settings, MCP servers, extensions, skills, prompts, themes and agents (`crates/psi-cli/src/trust.rs`), so a change asks again before anything loads. `/reload` doesn't check it (`Worker::reload` in `crates/psi-cli/src/interactive/worker.rs` reloads whatever is there), and a project with no `.psi/` at startup counts as having nothing to trust, so a `.psi/extensions/` created during the session loads on `/reload` without a question. Options:
  1. **`/reload` checks the fingerprint (done 2026-11-03).** If `.psi/` changed since it was trusted, show the same question as at startup (as a card, listing the changed files) before loading; no answer, no load. Small, covers every way `.psi/` can change (bash, approved file-tool writes, a `git checkout`, the user's editor), and makes §9.3's "again whenever they change on disk" true mid-session.
  2. **Read-only `<root>/.psi` in the sandbox**, as `~/.psi/` already is: a read-only bind after the workspace bind in bubblewrap, a `file-write*` deny in Seatbelt. Then the file tools, which ask, are the only way in. Closes bash's way entirely, but a missing `.psi/` can't be bound read-only (bubblewrap needs the path), so a project without one stays open unless Psi creates the folder or hides the path; and it does nothing about other changes on disk.
  3. **The plan's checkpoint-diff re-trust (§13):** after a turn, a checkpoint diff that touches `.psi/` holds settings and extensions until the user approves again. The fuller form of option 1, waiting in §13.
  4. **A warning only:** when a turn's checkpoint shows `.psi/` changed, say so on screen. Cheapest, and leaves the decision to the user at `/reload`.

  Options 1 and 2 together would close it: 2 stops the model's quiet way in, 1 catches everything else. Option 1 is built: `/reload` shows a card (y load it, n don't reload) when the project's `.psi` doesn't match what was trusted, including one created mid-session; the card names what `.psi` holds, not the changed files. Option 2 is still open.
- **Always-ask commands in a file (accepted, §8.3).** The classifier reads the command text (with `sh -c`, `eval`, wrappers and substitutions), not files: a script written first and run after (`bash push.sh`, `make deploy`, `npm run x`, `python x.py`) runs what is inside without asking. The plan accepts this ("dynamic forms run; the boundary contains them"). Worth knowing which credentials the sandbox can reach, since that decides what a hidden `git push` can do.
- **Gitignored files (medium).** write and edit ask before changing one, since checkpoints skip it; bash doesn't, so a clobbered `.env` can't be undone.
- **Read before overwrite (low).** The file tools refuse to replace a file the model hasn't read; bash doesn't check. Checkpoints still undo it.

### Plan mode's read-only check is a parser, not a boundary (added 2026-11-05, low priority)

In plan mode (and for read-only sub-agents) bash runs in the normal sandbox, which can write the workspace; only `read_only::check` keeps it from writing. The 2026-11-05 review's bypasses are fixed (options after operands, `rg --pre`, `fd -x/-X`, `awk -i/-l`, `bat --pager`, git words that expand, sed and awk scripts that expand). Still open, by choice:
- **An option inside a variable for a tool other than git:** `x=-i; sed 's/a/b/' $x f` passes, since a word that expands is taken for an operand. Refusing every such word would also refuse `sed -n 1p "$f"` in a loop, which models write.
- **Holes nobody has found yet.** Checkpoints undo a write to a tracked file.

**The fuller fix:** run plan-mode bash in a read-only sandbox (a read-only bind of the workspace in bubblewrap, a `file-write*` deny in Seatbelt). It needs a plan edit (§8 and the plan-mode section) and does nothing when isolation is off. Worth it if a model is seen getting past the check.

### Failure guards miss failures that aren't in a row (added 2026-11-05, medium priority, to implement)

From the 27B code eval (`~/.psi/evals/2026-11-05-1321-code-all-qwen3.8-27b`):
- `sh_manifest` (2/9, step cap): about 20 steps of one-line `bash -c` probes, each slightly different, nearly all failing with `$3: unbound variable`. `error_streak` (10 failed calls in a row) fired only at step 39, since a few calls in between succeeded and reset the count. `repeated_failure` didn't fire, since the calls differed.
- `feat_lru_cache` (PASS): `python3 /tmp/check_lru.py` run 5 times unchanged after `No module named 'kvcache'`. The warning at 3 ("change the call or try another approach") didn't change the call; the stop at 5 ended it, after the work was done.

- The cog session with the 27B (`~/.psi/sessions/-home-ndari-projects-cog-/2026-11-05-1547_e61d66e2.*`, the first extension prompt): after about 40 steps of exploring, more than 110 steps of read-only commands that all succeed, cycling through a few near-identical ones (`go run ./cmd/cog --docs | grep -E "^\| (swap|dup)"` 23 times, with `(swap|dup) ` and `(swap|dup|drop|over|nip) ` variants; `grep -n "func (e \*Evaluator) Eval(" -A 5`, `-A 6`, `-B 3`...). Every reply was a bare tool call with no text; no file was written. The results were all in context (40K of 64K, nothing cleared). `no_progress` fired only at steps 125 and 138: any call not seen before counts as progress, and a variant came every 2 to 4 steps. It only nudges, and the TUI has no request cap ("the guards still pause it", `run_mode.rs`), so nothing ever paused the run. The failure guards can't help, since nothing failed.
- The cog review with the 27B and clearing on (`2026-11-05-2125_d1ba1550.*`): the same two commands in turn, word for word, 25 times each. `no_progress` fired every 6 turns, 8 times, and the model ignored each nudge; only the owner's message stopped it. Exact repeats, so even the current rule saw them; what was missing is the pause.

**To build:** count failures in a recent window instead of in a row, e.g. 8 of the last 10 tool calls failed, so a success now and then doesn't reset it. Maybe make the warning name the error it repeats. For `no_progress`: count a turn as progress only if a call returned output not seen before (a variant with the same output is a repeat), or a file changed; and if the turns after a nudge still make no progress, pause and ask, as the failure guards do. Pick the numbers from the transcripts above and the recorded fixtures; one run, so keep them conservative.

**What others do** (source read 2026-11-05: pi `35dfa75`, codex `49f0c60`, opencode `d8cb7c7` and the legacy runtime at `bb15b16`): pi and codex have no loop or failure detection and no step limit. Opencode's legacy runtime had a "doom loop" check: the last 3 tool parts of the assistant message with the same tool and identical input (errors not required) ask the user, and a no ends the turn. It never messages the model, and since each step is a new message it likely caught only repeats inside one response. Its current runtime keeps only the setting. Its optional step limit tells the model tools are off and asks for a text-only answer. So there is nothing to copy; Psi's guards already go further.

**Test together:** one quick-set run once this is built, which also checks the 2026-11-05 denial wording (parse errors say to fix and run again), not yet run on a model.

### A summary can contradict what the harness recorded (added 2026-11-08, medium priority, to implement)

After a compaction the model gets its own summary and the harness's record (`harness_state` in `crates/psi-agent/src/compact.rs`: files read, files changed, todo, last post-edit check). When the two disagree, the model follows the summary.
- **The cog review, old build** (`~/.psi/sessions/-home-ndari-projects-cog-rv-old3-/2026-11-06-2002_c5f4f790.*`): the second summary copied the first word for word. It said "Have NOT yet read: errors.go, source.go, options.go, word_metadata.go, stdlib.go..." and "Have NOT run: go build, go test", and under Corrections, "None to the prior summary". Calls #24 and #25 had built and run the tests, and #32 to #36 had read those files. The record beside it listed them under "Files read". The model rebuilt, re-read #32 to #36 exactly, and re-read three more files: about 12 calls.
- **The 11-05 cog session** (`2026-11-05-1547_e61d66e2`): a summary after #166 said "I have NOT yet written the extension file", though #156 had. At #405 the model rediscovered its own file.

**What the record leaves out:**
- commands run through bash and their exit codes (builds, tests, `go run`);
- files read or written through bash (`cat`, `sed -n`, a heredoc).

The 27B reads and writes through bash often (the cog runs on 2026-11-06).

**To build:**
- Record the bash commands run, the last few distinct ones, each with its exit code.
- Give the summarizer the harness record, so it doesn't write "not read" or "not run" for what the record shows.
- When a summary still contradicts it, drop or correct the contradicting line before the model sees it.

All three change model-facing text, so each needs an eval run. The cog review and the 16K tasks are the cases to rerun.

### Tool-call markup inside an argument (added 2026-11-04, low priority)

In an eval trial the 35B wrote a todo call whose `action` held the next parameter too: `"add>\n<parameter=text>\nCreate textkit/ package ..."` (llama-server's parser for Qwen's XML tool calls took a stray `>` as part of the value; `~/.psi/evals/2026-11-04-0236-quick-batteries-agents-qwen3.6-35b/multi_split_package`). The todo tool refused it as an unknown action and the model redid it correctly in the next call, so it cost one request. Fix if it comes back: in `psi-ai`'s shape fixes, split a string argument holding `\n<parameter=NAME>\n` into that parameter, with this call as the fixture.

### No guard notices reading without fixing after a failing test (added 2026-11-05, low priority, to discuss)

In the Qwen3.8 27B extension run (`~/.psi/evals/2026-11-05-0337-h01-ext-qwen3.8-27b-full/ext_bookmarks`) one of the model's own tests failed at step 5; it then spent the 15 steps left reading example extensions (`psi ext example ...`, one command three times) and never edited again, so the run ended at the step cap. No guard fired, as designed: the repeat guard counts only failed calls, and the no-progress guard (G-12) needs 6 turns that only repeat earlier calls, while most of these reads were new. The 35B wrote 33K tokens on `ext_todo_scan` in a 2026-11-01 run without a guard either. A possible guard: after a failing check, N turns that only read and change no file get a nudge naming the failure ("The last check failed: <first line>. Fix it, or say why you can't."). Headless only, like the answer guard, since a person can step in. One run, one model; worth it only if it shows up again.

### `path:line` in answers isn't clickable (added 2026-11-04, low priority)

Left over from the markdown work (plan §11.2, `psi-cog-test-issues.md`, "Answers look flat"): a code span like `cmd/cog/repl.go:92` in an answer could be an OSC 8 `file://` link to the file. The line can't go in the link (plan §15 item 5), so a click opens the file at the top; tmux needs `set -as terminal-features ',*:hyperlinks'`. Links already work for markdown links.

### The user's words are hard to read on a light terminal (added 2026-11-03, low priority)

Committed user messages are bright white and bold (`commit_user` in `crates/psi-cli/src/interactive/ui.rs`, plan §11.2), which stands out on a dark background but nearly disappears on a white one. On a light theme they should be dark instead (black, or the terminal's own foreground, bold). Options: a theme token for the text (pi's themes have `userMessageText`, which Psi uses for the `▌` bar), or picking by the terminal's background (OSC 11 query, or `COLORFGBG`). Low priority: the owner uses a dark terminal.
