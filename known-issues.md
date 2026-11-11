# Known issues

Problems we know about and have chosen not to fix yet, with how much they matter. When one is fixed, remove it here and say so in the commit.

## Open

### What "nothing the owner can't recover from" doesn't cover yet (added 2026-11-09, with step 2 of `design-going-forward.md`)

- **The macOS keychain (high on the Mac).** A token kept in the login keychain is not hidden by a file deny for certain (`Library/Keychains` is on the list; the keychain is also reached through `securityd`). Git is covered another way: bash gets an empty `credential.helper`, so git asks no helper. (Whether that keeps a keychain token from git could not be shown over SSH on 2026-11-09: the keychain gave nothing there with or without it. Try it in a terminal on the Mac.) Other tools that read the keychain (`gh` with keychain storage, `security find-generic-password`) are not. Needs a `mach-lookup` deny tried on the Mac.
- **The SSH agent socket on macOS (medium).** `SSH_AUTH_SOCK` is removed from bash's environment, but launchd's socket is under `/private/tmp`, which Seatbelt leaves readable and writable, so a command that looks for it can use the agent. On Linux the socket is under the runtime directory, which is hidden. Needs a `network-outbound` deny on the socket's path, tried on the Mac.
- **Gitignored files in a directory ignored as a whole (low).** Checkpoints capture ignored files up to 1 MB each (2,000 at most), but not inside a directory that is itself ignored (`target/`, `node_modules/`, also a `secrets/` folder), and not larger ones. A clobbered file there can't be undone.
- **Git objects (low).** Checkpoints record branch and tag tips and HEAD, not the commits. `/undo` after a rewrite relies on the old commits still being in `.git`, which holds unless something prunes them (`git gc --prune=now`, `git reflog expire`). Removing `.git` and dropping a stash are refused; a stash's contents are not recorded.
- **A branch made during a run stays after `/undo` (low).** Only refs a checkpoint recorded are put back; nothing is deleted.
- **A failed checkpoint doesn't stop the run (low).** It is recorded as a guard event and the run goes on, so changes after it can't be undone until a capture works again. File writes used to ask in that state.
- **The refuse list reads the command, not files (accepted).** A script written first and run after (`bash push.sh`, `make deploy`) runs what is inside. The list is a courtesy; hidden credentials are what stop a push.
- **Git hooks and config written during a run (medium, from the 2026-11-05 code review).** bash can write the project's `.git`, so a hook or a config entry the model wrote there runs on the owner's next git command, outside the sandbox. Psi's own git calls ignore hooks. Checkpoints don't record `.git/hooks` or `.git/config`. A read-only bind for those two is the candidate.
- **A workspace above the home directory (low, same review).** `/` and the home directory are refused as the workspace; a parent of home such as `/home` is not, and would make `~/.psi` writable.
- **Read before overwrite (low, from 2026-11-03).** The file tools refuse to replace a file the model hasn't read; bash doesn't check. Checkpoints still undo it.

### Plans and tasks: what the first version leaves out (added 2026-11-09, with step 3 of `design-going-forward.md`)

- **A failed check at the end of a task stops the plan without another try (medium, from use).** The model saw the check fail after its edits during the task (the post-edit check), but when the last change came from bash, or the model stopped early, the end-of-task check is the first it fails. Running the plan again starts the item over in a fresh context, on top of the files as the failed task left them.
- **The record of earlier items is per session (low).** A task's prompt lists what earlier tasks of the same session changed and whether the check passed. Items done in another session show only as ticked boxes in the plan.
- **After `/run`, the chat goes on in the last task's context (low).** The conversation before the plan run is not sent any more (it is still in the session). Good for "fix what that task left", surprising for "as I said earlier".
- **`/undo task <n>` takes back the tasks after it too (by design, low).** It restores the checkpoint at task n's start; taking one task out from under later ones would need a merge. The conversation is not moved: the next prompt still follows the last task's context.
- **The limits are starting values (to set from use).** 40 requests, 200 tool calls and 2 summaries per task (`limits`), from the old print-run defaults.
- **One summary at a time (accepted).** A new summary takes the earlier one's place, so what the model leaves out is gone from the context (the session keeps it). The design gives up faithful summaries over many compactions; a task that needs a third stops.
- **Names from before (low).** `GuardAction::Pause` and the `*_PAUSE` constants stop the run now; `Activity::Waiting` in `psi-tui` is unused.

### Trying a model: what is not there yet (added 2026-11-09, with step 4 of `design-going-forward.md`)

- **The trial set has one real prompt (medium, grows with use).** `cog_bare_words` is a prompt from daily use on cog, started from a branch of the local repository; the other eight are existing tasks with the most to read and change. More real prompts on other projects should join it (`evals/README.md`, "A task from a real project"). The cog task (also as a plan, `cog_bare_words_plan`) works only on a machine with `~/projects/cog`. Both were run once on 2026-11-10 and failed without an edit (`experiments.md`).
- **`psi guards` reads what the session recorded (low).** A call's identity comes from the recorded arguments, which for a call the harness refused before running are not the ones the live guards saw; a session from before 2026-11-09 doesn't say whether a person was at the keyboard, and is taken as unattended (the `announced_stop` nudge may show where it did not apply). Its stops were not compared one by one with the 2026-11-09 script's (18 runs of about 800 then; 17 no-progress stops in 520 runs on this machine now, which holds only part of those recordings).
- **`psi replay --count` matches text (low).** The rule to judge an edit by is "the next action contains this text"; anything finer still needs reading the tally.
- **Model runs go one request at a time (as before).** `--samples` sends them in turn; the server has one slot.

### Guards miss loops that aren't exact repeats, and failures that aren't in a row (added 2026-11-05, medium priority; the `no_progress` stop built 2026-11-09)

**Built 2026-11-09:** 12 turns in a row that only repeat earlier calls and change no file now stop the run, after the one nudge at 6 (`psi-plan.md` §8.6). At the first stop the model gets a note that names the repeated calls and the run goes on once; the second stop ends it. Replayed over the recorded sessions, it stops the two exact-repeat sessions below (the 11-05 clearing-on review at turn 36 of 110, the 11-06 TUI rerun at turn 96 of 708) and 7 of the 8 loops in `experiments.md`. Since the same day every run also has limits on requests, tool calls and summaries, so a loop the guards miss ends at a limit.

**Still open after that:**
- The failure window ("To build", below): unchanged.
- Probes that differ a little each time and all succeed: `sh_archive_fix` in the quick set of 2026-11-10 (`~/.psi/evals/2026-11-10-0952-quick-after-step5-qwen27b`) rewrote its script at request 2, then ran 38 `find -mtime` probes in `/tmp`, two alike, until the request limit. In the same run `feat_query_paging` and `multi_ini_to_json` repeated exactly and got the nudge and the note; the note produced one edit in the first, and the limit ended both.
- A marker that changes with each repeat: old-build run 1 of 2026-11-06 (`2026-11-06-1728_963ab284`) ran one `grep` 90 times, most with `; echo MARK_XYZ`, `; echo MARK99`, `; echo ===Z===` added, so every call was new to the guard. Catching it means reading the command (dropping a trailing `echo` of a literal), not comparing output: the marker is in the output too.
- A scratch file whose name changes with each repeat: the 27B's bare-words session on cog in pi, 2026-11-10 (`experiments.md`, "A standing anti-loop rule"), five times in one session and 94 of its first 245 calls, the longest 55 in a row: `cat > /tmp/rc.cog <<'EOF' ... EOF; go run ./cmd/cog /tmp/rc.cog`, then `rd`, `re` and so on with the same program. The name is in the error output too, so in three of the five no output equalled an earlier one either. Like the marker, it means reading the command: the same text once the `/tmp` name is put aside.
- A lookup whose flags change (`-A 5`, `-A 6`, the 11-05 15:47 session below): a new variant every 2 to 4 turns starts the count over. Counting output seen before doesn't catch it either, since each variant prints something a little different. A window (most of the last N turns were repeats) would, but an edit-then-test cycle repeats its test command every other turn, so it first needs "repeat" to mean "since the last file change".
- A cycle that changes files and changes them back in separate calls: each turn's checkpoint sees a change. It needs the checkpoint's tree compared with earlier ones.

From the 27B code eval (`~/.psi/evals/2026-11-05-1321-code-all-qwen3.8-27b`):
- `sh_manifest` (2/9, step cap): about 20 steps of one-line `bash -c` probes, each slightly different, nearly all failing with `$3: unbound variable`. `error_streak` (10 failed calls in a row) fired only at step 39, since a few calls in between succeeded and reset the count. `repeated_failure` didn't fire, since the calls differed.
- `feat_lru_cache` (PASS): `python3 /tmp/check_lru.py` run 5 times unchanged after `No module named 'kvcache'`. The warning at 3 ("change the call or try another approach") didn't change the call; the stop at 5 ended it, after the work was done.

- The cog session with the 27B (`~/.psi/sessions/-home-ndari-projects-cog-/2026-11-05-1547_e61d66e2.*`, the first extension prompt): after about 40 steps of exploring, more than 110 steps of read-only commands that all succeed, cycling through a few near-identical ones (`go run ./cmd/cog --docs | grep -E "^\| (swap|dup)"` 23 times, with `(swap|dup) ` and `(swap|dup|drop|over|nip) ` variants; `grep -n "func (e \*Evaluator) Eval(" -A 5`, `-A 6`, `-B 3`...). Every reply was a bare tool call with no text; no file was written. The results were all in context (40K of 64K, nothing cleared). `no_progress` fired only at steps 125 and 138: any call not seen before counts as progress, and a variant came every 2 to 4 steps. It only nudged then, and the TUI's chat has no request limit, so nothing stopped the run. The failure guards can't help, since nothing failed.
- The cog review with the 27B and clearing on (`2026-11-05-2125_d1ba1550.*`): the same two commands in turn, word for word, 25 times each. `no_progress` fired every 6 turns, 8 times, and the model ignored each nudge; only the owner's message stopped it. Exact repeats, so the rule saw them; what was missing then is the stop.

**To build:** count failures in a recent window instead of in a row, e.g. 8 of the last 10 tool calls failed, so a success now and then doesn't reset it. Maybe make the warning name the error it repeats. For `no_progress`: count a turn as progress only if a call returned output not seen before (a variant with the same output is a repeat), or a file changed. (In the cog plan run of 2026-11-10 that rule would not have fired: the searches that found nothing new came at most 3 in a row, `experiments.md`.) Pick the numbers from the transcripts above and the recorded fixtures; one run, so keep them conservative.

**What others do** (source read 2026-11-05: pi `35dfa75`, codex `49f0c60`, opencode `d8cb7c7` and the legacy runtime at `bb15b16`): pi and codex have no loop or failure detection and no step limit. Opencode's legacy runtime had a "doom loop" check: the last 3 tool parts of the assistant message with the same tool and identical input (errors not required) ask the user, and a no ends the turn. It never messages the model, and since each step is a new message it likely caught only repeats inside one response. Its current runtime keeps only the setting. Its optional step limit tells the model tools are off and asks for a text-only answer. So there is nothing to copy; Psi's guards already go further.

### A summary can contradict what the harness recorded (added 2026-11-08, low priority since 2026-11-09)

`design-going-forward.md` gives up faithful summaries over many compactions: a task is meant to fit one context, a run gets 2 summaries and then stops. So this matters less than when it was written, and stays here for the runs that do reach a summary.

After a compaction the model gets its own summary and the harness's record (`harness_state` in `crates/psi-agent/src/compact.rs`: files read, files changed, last post-edit check). When the two disagree, the model follows the summary.
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

All three change model-facing text, so each needs a replay with samples first and then an eval run.

### `/diff` is hard to use in a long session (added 2026-11-03, needs the owner's view of what's wrong)

The owner found `/diff` not right in the cog trial. It shows the diff between each pair of checkpoints along the session, in order, each labelled by its source. Since 2026-11-09 `/diff task <n>` shows one task of a plan. Still to settle: the whole session as one diff by default, what changed since the last prompt, one file's changes (`/diff <path>`), and telling committed work from pending. The first notes are in `docs/history/cog-trial-issues-2026-11-03.md`.

### From the code review of 2026-11-05: open, on code that still exists (low unless noted)

As found at `d6419be` and not checked again since, except where it says so; look before fixing. The review is `docs/history/code-review-2026-11-05.md` (the ids are its own).

- **C12, medium. A cut line in the session file or the wire log.** One partial line makes the wire log unreadable to replay; a partial session append leaves bytes the next entry is glued onto, and `branch()` stops quietly at the missing parent.
- **No idle timeout on the stream (C16; checked 2026-11-10).** `client.rs` sets only a connect timeout, so a server that stalls mid-answer blocks a print run.
- **P2. A failed `edit` on a big file can take many seconds** (the closest-match search runs over every window), and abort can't stop it.
- **C15. Edit's whitespace-tolerant match:** a deletion leaves a blank line, and new text already in CRLF gets `\r\r\n`.
- **C16, smaller ones:** `announced_stop` fires on "Let me know if you want changes."; results held back by the step budget say "Not run" although the calls ran; the grader counts every `PASS ` line, repeats included; a trial takes the first `.jsonl` it finds as the session; `/new`, `/resume` and `/fork` keep the old session's status; entry ids are 32 random bits with no collision check.
- **P4.** `read` loads the whole file before cutting it; `!cmd` output has no cap; the checkpoint store is never packed or pruned.
- **D1, D2. `run.rs` and the TUI's `App` are large state machines kept in order by hand** (`run.rs` is 1,134 lines after the cuts). **D5.** The panic hook is process-wide, so a panic in the worker turns raw mode off under a live UI.
- **`branch()` is quadratic** (checked 2026-11-10: `store.rs` scans for every parent), and `psi-agent` returns `Result<_, String>` where library crates should use `thiserror` (both from `fable-suggestions.md`).

### Tool-call markup inside an argument (added 2026-11-04, low priority)

In an eval trial the 35B wrote a call to the todo tool (cut since) whose `action` held the next parameter too: `"add>\n<parameter=text>\nCreate textkit/ package ..."` (llama-server's parser for Qwen's XML tool calls took a stray `>` as part of the value; `~/.psi/evals/2026-11-04-0236-quick-batteries-agents-qwen3.6-35b/multi_split_package`). The tool refused it as an unknown action and the model redid it correctly in the next call, so it cost one request. The parser slip can hit any tool. Fix if it comes back: in `psi-ai`'s shape fixes, split a string argument holding `\n<parameter=NAME>\n` into that parameter, with this call as the fixture.

### No guard notices reading without fixing (added 2026-11-05, to discuss; seen again 2026-11-10)

**Seen again 2026-11-10** on ordinary work: both cog trials ended without an edit, the plan's first task after 40 requests of reading. In a replay, one line saying no file is changed yet made 18 of 20 samples edit (`experiments.md`, "The cog task as a plan"). A note for a plan's task that has only read is the candidate; it is new model-facing text and waits for the owner.

In the Qwen3.8 27B extension run (extensions are cut since) (`~/.psi/evals/2026-11-05-0337-h01-ext-qwen3.8-27b-full/ext_bookmarks`) one of the model's own tests failed at step 5; it then spent the 15 steps left reading example extensions (`psi ext example ...`, one command three times) and never edited again, so the run ended at the step cap. No guard fired, as designed: the repeat guard counts only failed calls, and the no-progress guard (G-12) needs 6 turns that only repeat earlier calls, while most of these reads were new. The 35B wrote 33K tokens on `ext_todo_scan` in a 2026-11-01 run without a guard either. A possible guard: after a failing check, N turns that only read and change no file get a nudge naming the failure ("The last check failed: <first line>. Fix it, or say why you can't."). Unattended runs only, like the answer guard, since a person can step in.

### `path:line` in answers isn't clickable (added 2026-11-04, low priority)

Left over from the markdown work (`psi-plan.md` §11.2): a code span like `cmd/cog/repl.go:92` in an answer could be an OSC 8 `file://` link to the file. The line can't go in the link, so a click opens the file at the top; tmux needs `set -as terminal-features ',*:hyperlinks'`. Links already work for markdown links.

### The user's words are hard to read on a light terminal (added 2026-11-03, low priority)

Committed user messages are bright white and bold (`commit_user` in `crates/psi-cli/src/interactive/ui.rs`, `psi-plan.md` §11.2), which stands out on a dark background but nearly disappears on a white one. On a light theme they should be dark instead (black, or the terminal's own foreground, bold). Theme files are cut, so the fix is in the one built-in look: pick by the terminal's background (OSC 11 query, or `COLORFGBG`). Low priority: the owner uses a dark terminal.
