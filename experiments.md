# Experiments to run

Questions put to a model or a machine, kept so they don't get lost. Each entry says what to find out, how, and what's still open. When an experiment is run, note the date, where the results are, and what came of it. A change that follows from one goes through `design-going-forward.md`: a harness change needs the problem seen more than once, on ordinary tasks.

Entries under "Done" are dated records and are left as written, so some name features that were cut on 2026-11-09 (extensions, sub-agents, MCP, clearing old results, the pi adapter) and cite sections of the old plan (`docs/history/psi-plan-2026-11-09.md`).

## Open

### llama-server flags per machine (added 2026-11-01)

**Question:** which llama.cpp settings give the best prompt and generation speed for each model in use on each machine (this Linux box, RTX 4070 Ti SUPER 16 GB; the Mac, M4 Max 64 GB)?

**How:** `llama-bench` (ships with llama.cpp) runs every combination of the values given and reports tokens per second for prompt processing and generation. Test at depth, since agent runs spend most of their time with 30K+ tokens in context:

```
llama-bench -hf bartowski/Qwen_Qwen3.5-4B-GGUF:Q4_K_M -fa on,off -ub 256,512,1024 -ctk f16,q8_0 -ctv f16,q8_0 -d 0,32768 -o md
```

Flags worth sweeping: `-fa`, `-b`/`-ub`, `-ctk`/`-ctv` (KV cache type), `-t`, `-ngl`, `-ncmoe` (MoE experts on the CPU, for the 35B).

**Open:**
- llama-bench doesn't cover speculative decoding (MTP), sampling or the server's prompt cache; those need llama-server runs (its timing logs, or an eval run).
- It measures speed only. A quantized KV cache (`q8_0`) can change output; check quality separately before adopting one.
- Run it when the GPU is otherwise idle, or both it and anything else running are skewed.
- Maybe a `just bench <model>` recipe once the useful flag set is known.

**Run 2026-11-03, prompt batch size for the 35B on the Mac** (llama-bench, build 8642541b2, the UD-Q4_K_XL file, `-fa 1 -ngl 99 -b 4096`, reading 4,096 new tokens on top of 0, 16K and 40K already in context, 2 repetitions; tok/s):

| `-ub` | depth 0 | depth 16K | depth 40K |
|---|---|---|---|
| 512 (default) | 1554 | 994 | 656 |
| 1024 | 1605 | 1052 | 666 |
| 2048 | 1681 | 1082 | 665 |
| 4096 | 1707 | 1075 | 617 |

2048 is about 8% faster up to 16K and the same at 40K; 4096 is slower deep in the context. Adopted: `-ub 2048` in the `qwen35b` recipe, and the Mac server restarted with it. The batch size isn't what slows reading late in a session: the speed falls with depth at every size (attention over the whole context), so the levers there are fewer tokens read (no line numbers, clearing less often) rather than server flags.

### write says what a replacement removed (added 2026-11-01)

**Question:** does the model keep code it didn't mean to drop if write's result names what the replacement removed? In `fix_retry_backoff` the 35B replaced `retry.py` with write and dropped the `retrying` decorator that `sync_job.py` imports, in both runs (`~/.psi/evals/2026-11-01-1638-screen-qwen3.6-35b`, `2026-11-01-1705-quick-g19-qwen3.6-35b`); the result only said "Replaced retry.py (29 lines)".

**How:** when write replaces a file, compare the definitions (`def`, `class`, `fn` and the like) in the old and new text and name the missing ones, for example "Replaced retry.py (38 → 29 lines). No longer defined: retrying. Put back anything removed by mistake." Then rerun `fix_retry_backoff` once.

**Open:**
- Model-facing text: a longer result on every replacement, and a nudge that may make the model restore code it meant to remove.
- Matching definitions across languages is a heuristic; a rename shows up as a removal.
- The owner isn't sure it's worth it yet.

### Not running an exact repeat (added 2026-11-09)

**Question:** when the loop note fails and the model makes the same call again, should the harness not run it, and say so with the output it gave before?

**Context:** "Does the loop note last?" (below). In run 1 the model sent one command 12 more times after the note, each a bare tool call, each with the same output. A refusal ("refuse with the exact fix") comes ahead of a nudge in the harness's order of choices, and it takes away the one thing the loop still gets: a fresh result.

**How:** sampled replay first, no build. Take the requests that followed the note in runs 1 and 4 (`2026-11-09-2210_5f3b3b8d`, `2229_9ae9e8e4`) and the 14 recorded loops, and replace the last repeated result with a refusal, for example "Not run: you ran this exact command N times and no file changed since. Its output was: (the earlier output, cut)". A few seeds each, against the request as recorded. The measure as before, fixed up front: repeat, new or answer, with the new calls listed.

**Open:**
- When it applies. Only after the note has failed (the next rung), or for any exact repeat with no file changed since? The second also refuses honest repeats: polling a log, a command whose output depends on the time or on something outside the workspace.
- "No file changed" is what the checkpoint sees, so a command that reads a gitignored file or the network can change its answer without it.
- The model may make the call with a marker added to get past the refusal, as one run did by itself on 2026-11-06 (`; echo MARK99`).

### Other sampling for a request that repeats (added 2026-11-09)

**Question:** when the answer to a request is an exact repeat of an earlier call, does sending the same request again with other sampling give a different call?

**Context:** in "Cutting a loop out of the context" (below), 2 of the 42 samples sent as recorded left the loop, with nothing changed but the seed. The loop runs use the card's thinking-off sampling (temp 0.7, top-p 0.8, top-k 20). Nothing is added to the context, so the prompt cache holds and a try costs only the generation (`fable-suggestions.md`, B5).

**How:** sampled replay on the 14 recorded loop requests, as recorded, with a few seeds at each of a few settings (the server's; temp 1.0; temp 1.0 with top-p 0.95), then with the loop note added. Tally repeat, new and answer, and list the new calls.

**Open:**
- Sampling goes out per request since 2026-11-09 (the profile's `sampling`), and `psi replay --set temperature=1.0` tries a setting on a recorded request, so this needs no build to try.
- A higher temperature may write worse calls or code; read the new calls, don't only count them.
- Draft acceptance with MTP falls as sampling loosens, so each try is slower.
- If it works, where it goes: before the nudge (the harness quietly asks again), or after the note fails.

### After a summary the model goes on with what the kept tail shows, not the summary's next step (added 2026-11-09)

**Question:** in the first trial of `cog_bare_words` (Qwen3.8 27B, `~/.psi/evals/2026-11-10-0338-cog-bare-words-qwen27b`, MODEL-FAIL, 118 requests, no file edited), both summaries ended with a sound next step (reproduce the case, then change `evalWord` in `env.go`) and the model took it neither time: it went back to reading docs, looped on seven of them until the loop note at request 49, and read code in small slices until the context filled a third time. Why does it not go on from its own summary?

**How (2026-11-09, `psi replay`, no script):** the request right after the first summary (wire request 29) holds the summary, the harness's record, the user's request repeated, and the kept tail: one call, a read of `plan/design-review.md`, and its result (29.5K characters, cut off with more to read). Sent again 10 times each way:

| The request | Next action, of 10 |
|---|---|
| as recorded | 10 read the rest of the same doc (`offset: 592`) |
| the tail removed (`--drop-last 2`) | 8 build cog and run the reproducer, 2 read `env.go`: the summary's next step |
| a line added after the tail (`--append "[Note from psi] Earlier messages were replaced by your summary above. Go on from its Next step."`) | 10 build cog and run the reproducer |

**So:** the kept tail decides. It is the latest thing in the context and shows the model in the middle of reading a doc, with a result that says there is more; the summary's plan is 10K characters above it. One line after the tail turns all 10 samples to the plan. Removing the tail does nearly the same and costs the tail's content.

**Open:**
- One step, one recording. The loop note looked as good in a one-step replay and held in 1 of 3 live runs ("Does the loop note last?", below). It needs a live run or two before it is believed, and the same check on another recording.
- Which fix, if it holds: the line after the tail (cheap; it keeps the tail, so the cache-reusing summary request and the tail's content stay), or no tail when its last result is a cut-off read. The design's rule is to work around first and change the harness when a problem comes back on ordinary tasks; this is the first ordinary task it showed on.
- The same trial is the case for plans: as one prompt the work filled the window three times. Try it as a plan (`/plan`, `/run`) before reading too much into the summary.

### The cog task as a plan: a fresh context, and the first task still never edits (added 2026-11-09)

**Question:** the same work as `cog_bare_words`, written as a plan of four steps and carried out with `psi run` (`cog_bare_words_plan`, the plan in `evals/code/cog_bare_words_plan/plan.md`): does a small task in a fresh context get to the change?

**Run 2026-11-09** (Qwen3.8 27B, the profile's sampling, 40 requests a task, `~/.psi/evals/2026-11-10-0442-cog-plan-qwen27b`): MODEL-FAIL, 1 of 3 checks. The first task used its 40 requests in 3 minutes, changed no file and was left open, so the plan stopped there.

- **The context stayed small:** 18.5K tokens at the end, no summary. That part of the bet held.
- **It had what it needed by request 15:** `evalWord`, `apply`, and `applyName` and `callExecutable` in `instructions.go`, which do what the step asks for.
- **Then it kept reading, and part of that was a hunt for a function that does not exist.** From request 24 to 40, 9 of the 17 requests searched for `evalBlock` (the code has `evalBlockBody`) with the pattern a little different each time, some exact repeats, each returning nothing new. The other 8, in between, read more builtins in `fn.go` (`makeCompose`, `makeDip`, `makeIf`, `makeRepeat`) and did return new lines.
- **No guard fired, and "a search that finds nothing new" would not have either:** the searches with nothing new come at most 3 in a row (requests 38 to 40), with reads of new code between them.

**Replays 2026-11-10** (`psi replay` on the trial, the profile `qwen27b`, 10 samples each; one step of one recording):

| Request | Change | Next action |
|---|---|---|
| 24 | none | 10 read or search more (1 for `evalBlock`), 0 edit |
| 30 | none | 10 read or search more (5 for `evalBlock`), 0 edit |
| 24 | note appended | 9 edit `env.go`, 1 reads |
| 30 | note appended | 9 edit `env.go`, 1 reads |

The note, as a user message at the end: "[Note from psi] This task has made N requests and changed no file. You have read enough: make the change now with edit, then run the check." (N = 23 and 29).

**Run 2026-11-10 with thinking on** (`qwen27b-t`: thinking on, the card's thinking sampling; `~/.psi/evals/2026-11-10-1213-cog-plan-qwen27b-t`), stopped by the owner at request 26 of task 1 after 54 minutes, so there is no grade:

- **It edited.** Whole-file reads in batches for 14 requests, the first edit to `env.go` at request 15, the build and tests at 16 to 18, a second edit at 21, and `bare_words_test.go` written at 23. Without thinking the task never edited in 40 requests.
- **It is slow:** about 2 minutes a request, against under 5 seconds without thinking.
- **It filled the window in one task:** the context passed the trigger at request 20 (39K tokens, from reading whole files), and the summary failed, cut off by the output limit ("Summaries without thinking", under Done).
- One run, not finished: it shows that thinking changes what the model does on this task, not how it ends.

**The same two requests with thinking on (2026-11-10):** the server restarted as `just llm-up qwen27b-t`, and the recorded requests sent with `--set 'chat_template_kwargs={"enable_thinking":true}' --set temperature=1.0 --set top_p=0.95 --set presence_penalty=0.0` (the card's thinking sampling), 10 samples each, nothing appended.

| Request | Next action |
|---|---|
| 24 | 10 read, search or run the tests (2 run `go build` and `go test`), 0 edit |
| 30 | 10 read, search or run the tests (1 runs `go test`, 0 for `evalBlock`), 0 edit |

- Thinking does not make it edit at these points: 0 of 20, as without it. The lookups differ (error handling, tests, `CallFrame`), and the hunt for `evalBlock` is gone at request 30.
- A sample generated 47 to 3,184 tokens (the server's log; a call alone is about 70), so several thought for one to two minutes and then read on.
- These requests carry a history written without thinking; a run with thinking from the start may not get here at all.

- The hunt for `evalBlock` is one draw at request 24 and half the draws once it has started. Reading on without editing is what the model does there every time.
- Told once that nothing is changed yet, it edits, in the function the step names. Whether the edit is right was not looked at.

**Open:**
- A note for a task that has only read, built into `psi run`. Not built: it is new model-facing text, and it only fits where a change is expected (a plan's task), not a question asked with `-p`. To decide: after how many requests (half the task's limit?), and whether once.
- The single-prompt run with the note after a summary was stopped at 27 requests, before any summary, so the note is still untested live.

## Closed without a run

Removed on 2026-11-10 with step 6 of `design-going-forward.md`; the full text is in the git history (before that date).

| Entry | Added | Why closed |
|---|---|---|
| Speculative decoding for Gemma | 2026-11-01 | One class of model: the small models are no longer tuned for. |
| A lower step cap for eval runs | 2026-11-01 | A task set can carry its own request limit (`@steps`), and the comparisons with pi that a changed cap would have broken are gone. |
| Sub-agent answers survive compaction | 2026-11-01 | Sub-agents were cut; a fresh context per task does that job. |
| Nudge when re-reading lines whose earlier read is gone | 2026-11-01 | A run gets 2 summaries and then stops, saying the work is too big for one run. |
| MCP in the eval, and language-server tools | 2026-11-01 | MCP was cut. Its one run: no MCP call in 12 trials. |
| The model card's sampling for the 35B | 2026-11-03 | Applied 2026-11-05 without the comparison; since 2026-11-09 sampling is in the model's profile. |
| An outline in place of a cleared read | 2026-11-05 | Clearing old results was cut. |

## Done

### Summaries without thinking (added 2026-11-05)

**Question:** with thinking on, can a summary request turn thinking off, so the model doesn't think before it summarizes, and still reuse the server's cache?

**Context:** the cog review with the 27B and thinking on (`qwen27b-t` flags, `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-05-2246_87bc74da.*`): its one summary, at 47.2K tokens, reused the cache for 44.5K and read the rest in 12 s, then wrote 4,834 tokens at 20 tok/s, 236 s. The summary is 8.2K characters, about 2,000 tokens, so most of the output was thinking. With thinking off the same review's summary took 94 s (1,595 tokens).

**The catch, from the 27B's template (Qwen3.8, read from `/props`):** thinking changes two places in the prompt. At the end, the generation prompt is `<think>\n` with thinking on and `<think>\n\n</think>\n\n` with it off; that part alone would keep the cache. But with thinking on (or not set) the template also puts a reasoning-effort line first in the system message, `xhigh` by default ("Reasoning effort is set to xhigh. Please think carefully through the task..."), so turning thinking off for one request changes the start of the prompt and the server reads all of it again: about 47K tokens, minutes at this model's reading speed, which costs more than the thinking saved. `reasoning_effort` low changes the same line, so it breaks the cache too.

**How, if worth it:**
- Check each model's template first: the 35B's (Qwen3.6) may differ only in the generation prompt. Where it does, send the summary request with `enable_thinking: false` and accept the small re-read at the end. Psi's `extends_request` and `cache_break` treat a different thinking setting as a break at the start (fit.rs), so they would need to know which part changes.
- Or ask for a short summary think: an instruction in the summary request ("don't think at length") costs nothing in cache, but the model may ignore it.
- Measure the summary time and its output tokens on a recorded request (`psi replay`), not in a run.

**Open:**
- Psi sends no `reasoning_effort`, so the 27B with thinking runs at its template's default, xhigh. Whether medium or low does as well on agent work with less thinking is a separate question; it changes the system prompt, so it would be set once per session, never per request.

**Built 2026-11-10.** In the cog plan trial with thinking on (`qwen27b-t`), the first summary was cut off by the 8,192-token output limit and failed, so the run went on without one. A summary request now goes out with `enable_thinking: false` whenever the profile has it on; everything else about the request is the same.

- **The cost, checked with `/apply-template` on the 27B:** with thinking on the system message starts with "Reasoning effort is set to xhigh. ..."; with it off that line is gone. So the summary request does not reuse the server's cache and the context is read again (about 200 tokens a second on the Mac). That is about what the thinking cost before, and the summary arrives.
- **Tried on the request that failed** (`psi replay` of wire request 20 of that trial with `--set 'chat_template_kwargs={"enable_thinking":false}'`, then the same body sent directly; the thinking sampling kept, one sample, seed 0): the summary arrived with no thinking (`reasoning_content` empty). It took 9.5 minutes: 257 s to read the 39.3K tokens again (153 tokens a second, nothing from the cache) and 312 s to write 6,664 tokens. The summary is 3,349 words where the instruction says under 936. Its last lines list about 18 test files as edited; the session shows two edits to `env.go` and one new file, so that list looks wrong (not checked line by line).
- **So:** the change does its one job, a summary that arrives. It does not make a summary cheap with thinking on, and nothing more is planned here: the design's answer to costly summaries is tasks small enough not to need one. Not seen inside a live run.
- **A way to keep the cache, not tried:** leave thinking on and end the request with an assistant message holding an empty think block. `/apply-template` renders that as a continuation with the system message unchanged (the think block appears twice). Whether the chat endpoint accepts it with thinking on, and whether the model then writes a summary, needs a replay.
- **Reasoning effort:** `chat_template_kwargs.reasoning_effort` changes the system line too ("low": "Keep your thinking brief and focused..."), so it is a per-session setting, as noted above. Psi sends none.


### Does the loop note last? (added 2026-11-09)

**Question:** the note that names the repeated calls changed the next call in a one-step replay ("Cutting a loop out of the context", below). In a live run, does the model stay out of the loop after it, or come back to it?

**Setup:** as the current-build runs of 2026-11-06 ("Did the changes since clearing went off bring the loop back?", below): the Qwen3.8 27B on the Mac (`just llm-up qwen27b`, 64K, thinking off, llama.cpp b12110), the `cog_run` extension prompt, headless (`psi -p - --max-turns 120 --trust-project`), each run in a fresh clone of cog at `991d099` (`~/projects/cog-note1` to `5`, removed after the runs). Psi is the working tree of 2026-11-09 with the note built, first on `PATH`, so the model's own `psi` calls are the same build. A headless run gets the note once at its first no-progress pause and stops at the second (plan §8.6).

**How:** 5 runs, one after the other. On 2026-11-06 this prompt looped in 3 of the current build's 5 runs, so some runs may never reach the pause; those say nothing about the note.

**The measure, fixed before any run.** A loop is as before: the pause's own rule, 12 turns in a row that only repeat earlier calls (exact, without `intent`) and change no file. For each run:
- whether the pause came, and at which call;
- the first call after the note: a repeat (equal to an earlier call) or new;
- whether the pause came a second time (the run then stops, `stop: guard`), and how many calls after the note;
- how the run ended: an answer, the guard, or the 120-request cap;
- whether an extension's `init.lua` was written under `.psi/extensions` (other than the two the clone starts with).

The calls after the note are listed for the reader. Whether the extension works is not scored: one run each, and this is about the loop.

**Run 2026-11-09** (`~/.psi/evals/2026-11-09-1610-loop-note-live-qwen3.8-27b`: each run's output, `runs.log`, the scripts; sessions under `~/.psi/sessions/-home-ndari-projects-cog-note1-` to `5`; 31 minutes in all):

| Run | Session (`2026-11-09-`) | The pause | First call after the note | Second pause | Ended | `init.lua` written |
|---|---|---|---|---|---|---|
| 1 | `2210_5f3b3b8d` | after call 55 | a repeat | yes, 12 calls later, all repeats | the guard, 67 calls, 4 min | no |
| 2 | `2214_895c2753` | never: the nudge after call 42, then it left the loop by itself | | | an answer, 49 calls | yes |
| 3 | `2219_0f7550e7` | never: no call repeated | | | the 120-request cap, 121 calls | no |
| 4 | `2229_9ae9e8e4` | after call 64 | new: one of the loop's two commands with `"` for `'` | yes, 13 calls later, 12 of them repeats | the guard, 77 calls, 7 min | no |
| 5 | `2236_e7fc5452` | after call 48 | new: a `write` of `.psi/extensions/cog-run/init.lua` | no: 9 calls, none a repeat | an answer, 57 calls | yes |

The calls, for the reader:
- **Run 1.** The loop was one command, `printf '5 :x let $x boom\n' | timeout 30 go run ./cmd/cog examples/factorial.cog; echo "exit=$?"`, which printed `{1 1 2 6 24 120} OK` and `exit=0` every time. The note named it. The next 12 replies were that command again, each a bare tool call with no text.
- **Run 4.** The loop was two `grep ... readString ...; sed -n '187,205p' lexer.go` commands in turn. After the note came the first of them with double quotes for single (new by the measure, the same lookup in fact), then the two again, 12 times.
- **Run 5.** The loop was seven `psi help api ...` lookups in varying order. After the note the model wrote "I have everything I need - writing the extension now." and called `write`. The file tool refused (`.psi` asks, and a headless run can't), so it wrote the file with a bash heredoc, added a test file, ran `psi ext test` and `psi ext check`, and answered.

**Outcome:** three runs reached the pause, so this is three samples of one task.
- **The note got one run of three out of the loop for good.** Run 5 went from seven lookups in turn to writing the extension and finishing.
- **Two went straight back,** one on its very next call, one after the same command with its quotes changed. The second pause stopped both, 12 and 13 calls after the note.
- **The one-step replay was too hopeful.** There the note changed the next call in 28 of 42 samples; here it changed what the run did in 1 of 3. In run 4 the next call did change, by the measure, and nothing else did: a new call is not a way out.
- **The stop is what bounds the cost.** Runs 1 and 4 ended at 67 and 77 calls, 4 and 7 minutes, where the 2026-11-06 runs that looped went on to the 120-request cap. Trying the note first cost them about 12 calls each.
- So the note stays as built: cheap, sometimes enough. It is not a fix for the loop.

**Seen on the way:** in run 5 `write` to `.psi/extensions` was refused and bash wrote the same file, the known gap ("Bash can write what the file tools guard", `known-issues.md`).

**Still open:**
- What to do when the note fails. Two candidates, both under Open: "Not running an exact repeat" and "Other sampling for a request that repeats". Run 1's replies were bare calls with identical output, so there is little for more words to work on.
- The same on ordinary coding tasks and another model (`fable-suggestions.md`, B2): every run here is the `cog_run` prompt.

### Cutting a loop out of the context (added 2026-11-09)

**Question:** when the model is stuck repeating calls, does removing the repeats from the context change what it does next, where the nudge and "Continue" don't? The repeated turns may be what primes the next repeat, and each nudge is one more line in that pattern (`fable-suggestions.md`, A2). The no-progress pause (plan §8.6, 2026-11-09) only stops the waste.

**Setup:** the Qwen3.8 27B on the Mac as on 2026-11-06 (`just llm-up qwen27b`: 64K, thinking off, the card's sampling, llama.cpp b12110). 14 recorded sessions where the pause would have come, all with this model and build: the 7 of 2026-11-06 that the rule pauses (`experiments.md` below) and 7 of 2026-11-08. The 2026-11-05 clearing-on review is left out (an older llama.cpp, and clearing on).

**How:** for each session, the request that followed the pause turn (the 12th turn in a row that only repeated earlier calls), from its wire log, sent again in four forms with seeds 1 to 3 each (nothing else changed but `max_tokens`: room for the session's longest earlier call, at least 300, so a repeat always fits whole and a response cut off by the limit is not one; it is counted by what it began):
- **full:** as recorded. This is what "Continue" on the pause card gives today.
- **full+note:** as recorded, plus a user message from the harness: "[Note from psi] You made the same calls again and again, the last 12 turns in a row, and no file changed. The calls: ... Don't make them again: use what you have, do something different, or answer."
- **cut:** the 12 repeating turns removed; the context ends where the streak began.
- **cut+note:** the 12 turns removed, plus the note, which also says the repeats were removed and that the calls' earlier results are still above.

**The measure, fixed before any run:** each sample is one of
- **repeat:** a tool call whose name and arguments (without `intent`) equal a call made earlier in the recorded session, up to the pause; a response with several calls is a repeat only if every call is one;
- **new:** any other tool call;
- **answer:** no tool call.

A "new" call may still be a variant of a repeated one (a flag changed); those are shown and left to the reader, not counted as repeats. One step only: this says whether the next call changes, not whether the model stays out of the loop.

**Before the runs:** one seed on the smallest session (`2026-11-08-2116_b9ef5c1a`) to test the script (as recorded: a repeat; the other three forms: a `write` of the extension, 660 to 815 tokens each). Seeing that a call that breaks out can be long, the seeds went from 5 to 3 and `max_tokens` from 1024 to the per-session cap, to bound the GPU time; the measure is unchanged.

**Run 2026-11-09** (`~/.psi/evals/2026-11-09-1427-loop-cut-qwen3.8-27b`: `samples.jsonl`, `run.log`, the script `cut_exp.py`; 168 samples, 82 minutes of server time). Each cell is repeat / new / answer of 3 seeds:

| Session | full | full+note | cut | cut+note |
|---|---|---|---|---|
| `2026-11-06-1507_9187a24e` (the TUI rerun) | 3 / 0 / 0 | 1 / 2 / 0 | 2 / 1 / 0 | 0 / 3 / 0 |
| `2026-11-06-1820_337ddaaa` | 3 / 0 / 0 | 2 / 1 / 0 | 1 / 2 / 0 | 0 / 3 / 0 |
| `2026-11-06-1904_4176ba71` | 3 / 0 / 0 | 3 / 0 / 0 | 3 / 0 / 0 | 1 / 2 / 0 |
| `2026-11-06-1736_ca300e42` | 3 / 0 / 0 | 1 / 2 / 0 | 0 / 3 / 0 | 0 / 3 / 0 |
| `2026-11-06-1812_45a2b294` | 3 / 0 / 0 | 2 / 1 / 0 | 3 / 0 / 0 | 1 / 2 / 0 |
| `2026-11-06-1827_7231f974` | 3 / 0 / 0 | 1 / 2 / 0 | 2 / 1 / 0 | 0 / 3 / 0 |
| `2026-11-06-1849_bc318ed3` | 2 / 1 / 0 | 0 / 3 / 0 | 3 / 0 / 0 | 0 / 3 / 0 |
| `2026-11-08-1841_b123cd94` | 3 / 0 / 0 | 0 / 3 / 0 | 0 / 3 / 0 | 0 / 3 / 0 |
| `2026-11-08-1854_d039213c` | 3 / 0 / 0 | 0 / 3 / 0 | 0 / 3 / 0 | 0 / 3 / 0 |
| `2026-11-08-2006_3a2c6732` | 2 / 1 / 0 | 2 / 1 / 0 | 3 / 0 / 0 | 1 / 2 / 0 |
| `2026-11-08-2019_1e222d3e` | 3 / 0 / 0 | 0 / 3 / 0 | 3 / 0 / 0 | 1 / 2 / 0 |
| `2026-11-08-2034_4dd3053d` | 3 / 0 / 0 | 2 / 1 / 0 | 3 / 0 / 0 | 1 / 2 / 0 |
| `2026-11-08-2046_5fa3abd5` | 3 / 0 / 0 | 0 / 3 / 0 | 3 / 0 / 0 | 0 / 3 / 0 |
| `2026-11-08-2116_b9ef5c1a` | 3 / 0 / 0 | 0 / 3 / 0 | 0 / 3 / 0 | 0 / 3 / 0 |
| **All 42 samples** | **40 / 2 / 0** | **14 / 28 / 0** | **26 / 16 / 0** | **5 / 37 / 0** |
| Sessions where all 3 repeated | 12 | 1 | 7 | 0 |

No sample answered: every one made a tool call. 38 of the new calls were cut off by `max_tokens`, every one of them the extension's `init.lua` being written (below); none could have been a repeat, since a repeat fits the limit.

**What the new calls were (read after the run; a description, not the measure).** All 14 sessions are the same task, the `cog_run` extension prompt, and in all of them the model was looking things up without having written the extension's `init.lua`. So one kind of new call can be counted by rule: a `write` of `.psi/extensions/<name>/init.lua`, or a bash `cat >` of it.

| | full | full+note | cut | cut+note |
|---|---|---|---|---|
| New calls | 2 | 28 | 16 | 37 |
| Start writing `init.lua` | 2 | 17 | 2 | 17 |
| `todo` | 0 | 2 | 0 | 1 |
| Other calls | 0 | 9 | 14 | 19 |

Samples of the other calls, for the reader to judge:
- cut, `1736_ca300e42`, where the loop had `psi ext example ask-first | sed -n '60,80p'` and `psi ext example fetch-cache | sed -n '50,62p'`: `psi ext example ask-first | sed -n '58,72p'`, `... | sed -n '45,62p'`, `psi ext example fetch-cache | sed -n '52,60p'`. The same lookups with the line ranges moved a little.
- cut, `1507_9187a24e`, where the loop had `grep -rn "func.*Text()" embedding.go; grep -n -B3 -A8 ") Text()" embedding.go | head -15`: the same command with `| head` added to its first half.
- cut, `2116_b9ef5c1a`: `go run ./cmd/cog -check examples/factorial.cog; echo "exit=$?"` twice, where the loop had the same check piped through `cat` or to `/dev/null`.
- cut+note, `1507_9187a24e`: `printf '2 3 + say\n' | go run ./cmd/cog; echo "exit=$?"` (trying the interpreter, a step toward the tool) and `todo add "Write .psi/extensions/cog-run/init.lua registering cog_run ..."`.
- cut+note, `2046_5fa3abd5`: `grep -n -B2 -A10 "func (e \*Evaluator) Eval(" evaluator.go; sed -n 41,50p evaluator.go`, where the loop had `-A8`: a flag changed.
- full+note, `2019_1e222d3e`: `printf '1 2 +\n' | bwrap --ro-bind / / ... -- go run ./cmd/cog` twice (trying the sandbox by hand), and a `cat > .psi/extensions/cog-run/init.lua`.

**What the cut cost in reading.** A cut request is the start of the request before it, yet in 11 of the 14 sessions the server reused only about 3.6K tokens and read the rest again: 15.3K tokens and 104 s on average for the first cut sample (up to 34K and 244 s), against 2.3K and 26 s for the first full+note sample. This fits "Prompt cache lost after a long final answer" below: llama-server can't cut this cache back to an arbitrary point and restores an earlier checkpoint. This replay sent each prompt whole, so the server had no checkpoint near the cut; a live session reads its prompt a turn at a time and may have one (not checked: it depends on how many checkpoints the server keeps, and the cut is at least 12 requests back).

**Outcome:** one step, one task and one model, so a direction, not proof.
- **"Continue" as it is today repeats:** 40 of 42. The pause stops the waste, and going on brings the loop back.
- **The note is what changes the next call:** with the note alone 28 of 42 samples left the loop, and 17 of them started writing the file the task asked for.
- **The cut alone mostly doesn't:** 26 of 42 repeated, 7 sessions repeated in every sample, and 14 of its 16 new calls were other lookups, several of them the loop's own commands with a range or a flag changed. Taking the repeats away puts the model back where it was when it started repeating. The idea that the repeated turns are what primes the next repeat (`fable-suggestions.md`, A2) is not what these runs show.
- **The cut adds a little to the note:** 5 repeats against 14, but the same 17 samples start writing the file either way, so the gain is in other calls, some of them variants. Against that, the cut costs a re-read where the note costs none.
- **So the candidate to build is the note, not the cut:** when the user answers "Continue" on a no-progress pause, a user message from the harness that names the repeated calls and says not to make them again. A headless run could get the same note once in place of stopping, and stop if the pause comes again.

**Still open:**
- Whether it lasts: tried the same day ("Does the loop note last?", above). Of three live runs that reached the pause, one left the loop for good and two went back to it.
- Another task and another model: all 14 sessions are the `cog_run` prompt on cog with the 27B (see the owner's rule that changes from the cog trial stay general).
- The note as a user message against the same words on the last tool result, where the nudge goes today. Only the user message was tried.

### Qwen3.8 27B on the Mac (added 2026-11-05)

**Question:** how does the dense Qwen3.8 27B (unsloth `Q6_K`, 22 GB, 64K context) do with Psi on the M4 Max, and how fast is it?

**Run 2026-11-05** (the extension eval, `just eval-ext`, one trial per task, 20-step cap). First `ext_lockfile_guard` alone, one setting changed at a time; all three passed 7/7:

| Server | Steps | Wall | Tokens written | Generating | Reading | Drafts accepted |
|---|---|---|---|---|---|---|
| Thinking (`reasoning_effort` medium), no MTP | 8 | 229 s | 2,772 | 17.8 tok/s | 190 tok/s | - |
| Thinking, MTP (`--spec-type draft-mtp --spec-draft-n-max 3`), `-ub 2048` | 9 | 196 s | 3,943 | 32.0 tok/s | 199 tok/s | 66% |
| Thinking off, MTP, `-ub 2048` | 8 | 113 s | 1,830 | 37.8 tok/s | 199 tok/s | 83% |

The `Q6_K` GGUF carries its own MTP layers (`blk.64.nextn.*`), so MTP needs no draft file. `-ub 2048` made no difference to reading here. The last setting is now `just llm-up qwen27b`.

Then the whole set, thinking off with MTP (`~/.psi/evals/2026-11-05-0337-h01-ext-qwen3.8-27b-full`), against the 35B's last run of it (`2026-11-01-1329-h01-ext-qwen3.6-35b-cap20`, also thinking off, with the Psi of that day):

| Task | 27B | 35B |
|---|---|---|
| ext_lockfile_guard | PASS 7/7, 8 steps | PASS 7/7, 14 steps |
| ext_trim_output | PASS 7/7, 13 steps | 3/7, 19 steps |
| ext_bookmarks | 2/7, step cap | 5/7, 10 steps |
| ext_todo_scan | 0/7, step cap (grader broken, below) | 5/7, step cap |
| Wall, all four | 538 s | 530 s |
| Tokens written | 10.7K | 49.4K |
| Generating | 33 tok/s (320 s) | 101 tok/s (490 s) |
| Reading | 162 tok/s (214 s) | 847 tok/s (36 s) |
| Drafts accepted | 82% | 67% |

**Outcome:** on the three tasks with a working grader, 16 of 21 checks for the 27B and 15 for the 35B, in about the same total time: the 27B is about 3x slower per token but wrote less than a quarter as many. Its time goes to reading prompts (a cold 11K-token first request takes 55 s), the 35B's to generating. The 27B was terse and went straight at each task; when stuck it explored instead of fixing (bookmarks: 15 steps reading examples after its own test failed; todo_scan: 7 steps of probe tests whose prints `psi ext test` hid). The 35B wrote more and finished more often, with wrong details. One run each, so no patterns.

Found by the run and fixed: the `ext_todo_scan` grader and reference used `os.execute` and `io.popen`, gone from extensions since `3db400e` (2026-11-02), so the reference scored 0/7 (`just eval-check --tasks all` now passes 45 of 45); `psi ext test` showed passing tests' prints only with `--verbose`, though the docs said it showed them, and now always shows prints. Noted in `known-issues.md`: no guard notices reading without fixing after a failing test.

**Code eval 2026-11-05** (every non-extension task, 41, one trial each, defaults: 40 steps, 64K; `~/.psi/evals/2026-11-05-1321-code-all-qwen3.8-27b`): 33 PASS, 8 MODEL-FAIL, 0 HARNESS-FAIL, 108 minutes, 168K tokens written. Six trials hit the step cap (two of them still passed). One run, so it proves the setup works, not how well the model does. What the transcripts show:
- **A false always-ask match cost `sh_precommit`** (9/11): testing its hook in `/tmp/hooktest`, the model's `git commit -m clean` was denied 5 times as `git clean`, since rule words matched in any order (§8.3). Fixed the same day: for git, an entry's first word must be the subcommand.
- **Repeating a failed call:** in `feat_lru_cache` (PASS) the model ran `python3 /tmp/check_lru.py` 5 times unchanged after `No module named 'kvcache'` (the script was in `/tmp`, not the project); the warning at 3 failures didn't change the call and the guard stopped it at 5, after the work was done. In `sh_manifest` (2/9) it spent about 20 steps on one-line `bash -c` probes failing with `$3: unbound variable`; the error-streak guard fired only at step 39, since a few calls in between succeeded.
- **The parse-error denial says "Don't retry it in another form"**, but for a quoting mistake (`sh_precommit` step 18, `sh_manifest` step 18) retrying with the quoting fixed is the right move, and the model did that. Changed the same day, not yet run on a model: without a user, a parse error says "Fix the command, or write it another way, and run it again", and other denials "Do without it, or say in your answer what you needed"; "Don't retry it in another form" stays only after the user's own no. The eval check waits for the guard work in `known-issues.md`.
- The other failures are wrong logic or a missed detail in the model's code, each after a check it judged enough (`feat_markdown_table` stopped after 5 steps with the padding wrong).

### The cog review in Psi and pi, and clearing off (added 2026-11-05)

**Question:** after two Psi sessions on cog with the 27B ended in loops, does the same review prompt loop in pi too, and what in Psi brings it on?

**Setup:** the Qwen3.8 27B on the Mac, 64K, with the card's thinking-off sampling (temp 0.7, top-p 0.8, top-k 20, min-p 0, presence penalty 1.5; `just llm-up qwen27b` since that day). The same prompt each time ("Take a deep look in this repo. Review what its trying to do, and review the design, architecture, testing setup, etc. report back your suggestions."), on cog with its `.psi` moved out and the leftovers of the earlier sessions removed. Neither harness sends sampling, so the server's applied to both. Pi 1.5 has no project settings; its `pi-llama` extension turns thinking from off to medium when it first reads the model's template (`index.ts:547`), which overrides `--thinking off` and `defaultThinkingLevel`; the thinking-off run set it back with Shift+Tab after a first "hi". The reviews are in `~/projects/pi-cog-review.md`, `pi-cog-review-no-thinking.md` and `psi-cog-review.md` (the owner's copies).

| | Psi, clearing on | Psi, clearing off | Psi, clearing off, thinking on | 35B in Psi, thinking on | pi, thinking off | pi, thinking on |
|---|---|---|---|---|---|---|
| Session | `2026-11-05-2125_d1ba1550` | `2026-11-05-2222_997e12eb` | `2026-11-05-2246_87bc74da` | `2026-11-05-2330_1c119ef2` | `2026-11-05T21-58-10-472Z_01a1adb8` | `2026-11-05T20-30-35-948Z_01a1ad67` |
| Time | 21 min, about 10 of it looping | 10.6 min | 32 min | 2 min 45 s | 12.5 min | 49 min |
| Tool calls | 136, 52 different | 36, all different | 72, 63 different | 39, all different | 44, all different | 66, 58 different |
| Longest run of repeated calls | about 50 | 0 | 3 | 0 | 0 | 3 |
| Context | 2 clearings at 46K (30K, 27K freed) | 1 summary at 46.7K | 2 summaries at 47.2K and 51.7K (249 s, 167 s) | 1 summary at 48.9K (31 s) | 1 summary at 57K | 3 summaries at about 50K |
| Guards | `no_progress` 8 times | none | none | none | none | none |

**What happened with clearing on:** the model read README, the plan docs and about 15 source files; at 15:30 clearing replaced about 30K of them with notes ("Result cleared to save context: ... Run it again if you need it."), before it had written anything. From 15:33 it ran the same two commands in turn, word for word, 25 times each; `no_progress` nudged 8 times and it ignored every one (the guard never pauses). A nudge from the owner ended it; asked why, the model said its reads had been cleared and it kept re-fetching them. The earlier cog session (`2026-11-05-1636_bc3c47c4`, before the sampling change) cleared at 11:57, and its probe loop began about 10 minutes later. The first loop that day (`2026-11-05-1547_e61d66e2`) had nothing cleared, though, so clearing is one trigger, not the only one.

**Outcome:** clearing is off by default (plan §7.4, `compaction.clear`). The rerun with it off went straight through, faster than pi. The reviews: pi with thinking reproduced each bug it named and ranked its fixes; Psi with clearing off found the same main bug (the bare `$` lexer cluster) and some neither pi run had (panics while loading the standard library, mixed error styles in builtins), with line numbers off and one wrong claim (`bracketsBalanced` "dead code"; `cmd/cog/editor.go` calls it); pi without thinking was decent; Psi with clearing on was the thinnest. One run each, so a direction, not proof.

**Psi with thinking on (added the same day):** the server with the `qwen27b-t` flags (the card's thinking sampling: temp 1.0, top-p 0.95, top-k 20, min-p 0, no presence penalty; reasoning effort the template's default, xhigh), clearing off, run headless (`psi -p --max-turns 200`) with the same prompt. No loop: the repeats are re-reads of `design-review.md` and `semantics.md` after a summary had replaced them. Its review was the deepest of the five: each bug reproduced with its output (the bare `$` and `:` cases), and findings no other run had (plan docs that describe removed features, `design-review.md` item 8 using `defsyntax`, which `TestNoAutomaticSyntaxExpansion` asserts is gone; stdlib words with placeholder signatures in `words.md`), plus a recommended order for the eight pending design items; three claims checked, all right. About 3x the time of thinking off, part of it in the summaries, which spent most of their output thinking ("Summaries without thinking", above).

**The 35B with thinking on (added the same day):** the owner's TUI run with the `qwen35b-t` flags (the card's general thinking sampling: temp 1.0, top-p 0.95, top-k 20, min-p 0, presence penalty 1.5; `preserve_thinking`), clearing off. The other columns are the 27B. It read in batches of 3 to 6 files a request, reached 45K by its 7th request (1.5 minutes in), was summarized while still reading, and answered from the summary and a few more test files: 11 requests, about 104 tok/s generating, and only about 1K characters of thinking in 7 blocks. It never ran the build or the tests (it twice thought "before running tests"), and never opened `plan/known-issues.md` or `design-review.md`. No harness problem: no guard events, no clearing. The review was long and well laid out but the thinnest in substance: no bugs found (it missed the bare-`$` cluster every 27B run found), the architecture described right, and several suggestions that don't fit the language (an empty `{}` contract for every block by default, `runtime.NumGoroutine()` to find scope leaks). The model's habit, not the harness: read everything fast, then answer without checking. Whether the card's precise-coding preset (temp 0.6, no presence penalty) changes that is untested.

**llama.cpp on the Mac updated 2026-11-05, after these runs:** from 0.8.0 (build 11960, commit `8642541b2`), which every run above used, to 0.9.0 (build 12110, commit `cd959bae9`), via Homebrew. Speeds and server behavior after this date are on 0.9.0, so compare them with these runs with that in mind.

**Still open:** `no_progress` should pause once a nudge goes unheeded and count exact repeats as no progress (`known-issues.md`); the sampling change alone didn't stop the loop with clearing on.

### Did the changes since clearing went off bring the loop back? (added 2026-11-06)

**Question:** in the cog rerun with the 27B (`~/.psi/sessions/-home-ndari-projects-cog-rerun-27b-/2026-11-06-1507_9187a24e.*`, TUI, default settings, clearing off), the first prompt ended in 623 calls of the same two `grep` commands in turn, with `no_progress` nudging 103 times; it was stopped by hand after 106 minutes. Clearing was off and nothing was cleared during the loop. Did the commits since `76fc900` (clearing off) bring the loop back?

**Setup:** the same server for every run (Qwen3.8 27B, 64K, thinking off, `just llm-up qwen27b`, llama.cpp b12110), headless (`psi -p - --max-turns 120`), each in a fresh clone of cog at `991d099`. Prompt 1 is the 11-03 script's first prompt (the `cog_run` extension), with `.psi` and `--trust-project`; the review prompt is the one from the table above, with `.psi` removed. "Old" is Psi at `76fc900` built from `git archive`; "current" is the working tree of 2026-11-06 (the C1 to C14 and P1 fixes). Its requests differ from the old build's only in the gopls tool schemas (sent whole, C9).

**What a loop is (the owner's definition):** the model clearly stuck, doing the same thing over and over in a row: the same exact call, or the same few exact calls, with nothing else between them. Repeats spread out between new calls don't count, nor do calls that change, such as a `sed` range moving forward through a file. `intent` is left out (the 27B rewords it for the same command), and so is an `echo` of a fixed marker added to the same command (`; echo ===A===`), which changes nothing it does. The same lookup with only its flags changed (`-A 5`, `-A 6`, `-B 3 -A 8`) also counts when nothing moves forward: the owner ruled the 11-05 15:47 run (`known-issues.md`) a loop, 113 of its last 114 calls being two lookups, 66 of them exact repeats.

**Prompt 1** (5 runs per build, plus the TUI rerun; every headless run stopped at the 120-request cap):

| Build | Run | Verdict | What it was doing | Session (`2026-11-06-`) |
|---|---|---|---|---|
| current | TUI rerun | loop | 2 exact `grep` commands in turn, 623 calls in a row from #87; `no_progress` 103 times | `1507_9187a24e` |
| current | 1 | no loop | 122 calls, none repeated | `1717_429f4341` |
| current | 2 | loop | one exact `go run ./cmd/cog <<'EOF' ...` call 103 times in a row (#20 to the cap) | `1820_337ddaaa` |
| current | 3 | no loop | 3 repeats, at the end | `1834_2d138f2f` |
| current | 4 | loop | wrote `.psi/extensions/cog-run`, then 2 exact calls in turn 85 times (#38 to the cap): write a probe test, run `psi ext test`, delete it | `1904_4176ba71` |
| old | 1 | loop | the same `grep` 90 times in a row (#32 to the cap), 66 of them with a marker `echo` added; same output each time, no guard fired | `1728_963ab284` |
| old | 2 | loop | 5 exact `psi ext example ... \| sed -n` commands, 79 in a row (#43 to the cap) | `1736_ca300e42` |
| old | 3 | loop | 2 exact `grep` commands, 29 in a row (#93 to the cap) | `1812_45a2b294` |
| old | 4 | loop | 4 exact `psi ext example` commands, 83 in a row (#39 to the cap) | `1827_7231f974` |
| old | 5 | loop | 7 exact `psi ext example` commands, 72 in a row (#50 to the cap) | `1849_bc318ed3` |

In old 2, 4 and 5 the few commands come in a varying order, not one fixed sequence, with nothing else between them.

**The review prompt** (3 runs per build; each wrote its review and ended by itself):

| Build | Run | Calls | Minutes | Verdict | Session (`2026-11-06-`) |
|---|---|---|---|---|---|
| current | 0 | 25 | 9.8 | no loop | `1748_07aa2326` |
| current | 1 | 32 | 10.2 | no loop | `1927_aad1238a` |
| current | 2 | 34 | 12.5 | no loop | `1950_d89eb475` |
| current | 3 | 48 | 12.6 | no loop (one dropped request, retried) | `2023_ce3201c3` |
| old | 1 | 28 | 10.0 | no loop | `1917_6fa756a5` |
| old | 2 | 48 | 12.3 | no loop | `1937_8b69e43b` |
| old | 3 | 73 | 20.5 | no loop: 18 repeats, one second pass over files already read, between new calls | `2002_c5f4f790` |

**Outcome:** the changes didn't bring the loop back. On prompt 1 the old build looped 5 times in 5 and the current one 3 in 5 (2 in 4 headless); on the review prompt neither looped in 3 runs, as on 11-05. Clearing off doesn't prevent the loop: no run cleared anything before it looped. The loop comes with prompt 1, where the 27B explores the extension API (`psi ext example ...`, Psi's docs and source) without writing; only one of the ten prompt-1 runs wrote code, and it then looped on probing its test. The old/current split is a few runs each, so a direction, not proof.

**A measuring mistake, kept as a lesson:** while scoring, a "near-repeat" count that ignored numbers was added after seeing a run, and it called a `sed` range moving forward through `env.go` a loop (current run 1); the owner caught it. The definition above was then fixed before scoring the rest.

**Still open:** the same as above: `no_progress` only nudges, so an unheeded loop runs until someone stops it.

### The small reference models without line numbers (added 2026-11-03)

**Question:** do Qwen3.5 4B and Gemma 4 12B cope with read output that has no line numbers (built 2026-11-03)? The eval after the change ran on the 35B only, and small models may lean on the numbers more: picking `offset`, or copying text for an edit.

**How:** one quick-set run per model (`just llm-up qwen`, `just llm-up gemma`), then read the transcripts for workarounds (`cat -n`, `nl`, `grep -n` to find lines), edit failures on copied text, and reads with wrong offsets. Compare with the Phase 2 runs (`2026-10-31-1634-psi-qwen3.5-4b`, `2026-10-31-1646-psi-gemma4-12b-qat`) for those signs, not for scores. Roughly an hour of GPU for both.

**Open:**
- If a small model needs them, numbers could come back as a per-model setting rather than for every model.

**Gemma run 2026-11-03** (`~/.psi/evals/2026-11-03-2119-quick-noline-gemma4-12b`, the Mac, 128K, one run, correctness only): 8 pass, 2 model failures (`multi_sqlite_migration` at the step cap, `feat_logstats_jsonl` 8 of 10), no harness failures. No sign it missed the numbers: no `cat -n`, `nl` or `grep -n` (one `grep -n` in the Phase 6 run), and no read used an offset. Its edits fail as often as before: 35 of 64 against 32 of 54 (Phase 6) and 12 of 23 (Phase 3), the same "old_text was not found" with the closest lines shown; it was Gemma's weak spot with numbers too. **Qwen 4B: won't do** (owner, 2026-11-03): a run takes over an hour on the Mac for little information, and the owner accepts that it, like Gemma and the 35B, does without the numbers.

### A quantized KV cache for reading deep in the context (added 2026-11-03)

**Question:** does an 8-bit KV cache (`-ctk q8_0 -ctv q8_0`) make the 35B read prompts faster deep in the context? The batch-size sweep showed speed falls with depth at every batch size (1,554 tok/s at 0, 656 at 40K), so the cost is attention over the cache, and a smaller cache means less memory to read per token.

**How:** llama-bench as in the batch run (`-ub 2048 -b 4096 -p 4096 -n 0 -d 0,16384,40000 -r 2`) with and without `-ctk q8_0 -ctv q8_0`. About 10 minutes with the server stopped.

**Open:**
- A quantized cache can change output; if it reads faster, check quality with an eval run before adopting it.
- Whether MTP drafting works with a quantized cache on this build.

**Run 2026-11-03** (llama-bench, build 8642541b2, `-fa 1 -ngl 99 -b 4096 -ub 2048`, reading 4,096 tokens and generating 128 at depth 0, 16K and 40K, 2 repetitions; tok/s; generation without MTP):

| K / V cache | read @0 | @16K | @40K | generate @0 | @16K | @40K |
|---|---|---|---|---|---|---|
| f16 / f16 (now) | 1779 | 1107 | 670 | 89.6 | 79.7 | 69.2 |
| q8_0 / q8_0 | 1598 | 1031 | 566 | 84.8 | 71.5 | 60.3 |
| f16 / q8_0 | 316 | 45 | 19 | 78.1 | 19.5 | 10.1 |
| q8_0 / f16 | 669 | 125 | 52 | 74.5 | 28.7 | 13.0 |

**Outcome:** no. An 8-bit cache is slower everywhere on Metal (reading 10 to 15% slower, generating 5 to 13%), and mixed types fall off a cliff (no fused kernel for them). The f16 cache stays; nothing changed. Memory isn't the constraint for this model at 64K, so the 8-bit cache has nothing to offer here.


### What the transcripts say about checking work and reading (added 2026-11-03)

**Question:** two things, from the recorded eval runs, no GPU: how often does the model give its final answer after editing without running anything since the last edit (the "Everything works correctly" habit), and how much of what it reads is read again, or whole large files?

**How:** a script over every `~/.psi/evals/*/*/trial-*/session.jsonl`: for each trial, whether files were changed, whether a bash call came after the last change, and the outcome; for reads, the share of read characters that were re-reads of a file already read (before and after a clearing or summary), and the share from whole-file reads of files over 200 lines.

**Run 2026-11-03** (291 trials in 46 eval runs from 2026-10-31 to 2026-11-03: Psi 259, pi 32; scripts were in the session scratchpad). A "change" is a successful write or edit (bash edits not counted); a check is any bash call after the last change.
- **Finishing without checking is rare.** The 35B in Psi checked after its last change in 159 of 165 trials that changed files; Gemma 27 of 33, Qwen 4B 40 of 46; pi's runs about the same. The few that didn't mostly failed (35B 1 of 6 passed).
- **The failures come from checks that pass.** Of the 35B's 43 failures (step-cap hits left out) that checked after the last change, 40 ended with a clean last run and only 3 with a failing one; pi's 35B 2 of 2 clean. The model tests what it thought of, not what the task asks, as in `feat_query_paging` (descending order with missing values). A "you didn't run anything" nudge would catch almost nothing; "your last command failed" would catch the 3. The real gap is weak self-tests, which a harness can't see without the hidden checks.
- **Reading at 64K is not where time goes in the evals.** Per trial the 35B read 11.5K characters in Psi and 13.7K in pi (Psi's still with line numbers then, which add about 15 to 20%). Re-reads of a file unchanged since the last read: 12% of Psi's reading against 4% of pi's; re-reads after the model's own change (checking an edit) 23%. Whole files over 200 lines: 30% in Psi, 55% in pi. The cog review's 113K against 65K was one interactive run in a much bigger repo, not the pattern here.
- **At 16K, re-reading is most of it:** 54% of the 35B's reading came after a clearing or summary removed the earlier read, 21% more were plain re-reads; that is the known loop (`multi_split_package`, `long_api_docs`).
- **The small models read more in Psi than in pi** (Gemma 14.0K against 7.6K characters per trial, Qwen 4B 13.1K against 8.4K, line numbers included), with more plain re-reads (22% against 16%, 13% against 6%). Those Psi runs are from Phases 2 to 6, and the task sets differ between runs, so this is a lead for the small-model run above, not a finding.

**Outcome:** no harness nudge for checking (it would rarely fire); reading at 64K is not worth changing the prompt for on eval-sized tasks. Worth knowing: the eval tasks' repos are small, so they say little about reading in a large real project like cog.


### Prompt cache lost after a long final answer (added 2026-11-02)

**Question:** why does the next turn re-read a long final answer instead of reusing it from the prompt cache, and can Psi avoid it?

**Context:** in a hands-on session in `../cog` (the 35B on the Mac, `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-0334_408b21c4.*`), two long answers that ended a turn (about 1,900 and 2,000 tokens) were each reprocessed whole on the next user prompt: `cache_n` fell about 1,905 and 2,030 tokens short of the previous request's prompt plus output. That put first token at about 3.5 to 4 s against about 0.3 s. Short final answers ("Done - ...", about 50 tokens) and assistant messages with tool calls were reused fully. The text Psi sent back is byte-identical to what was streamed. Pi, against the same server (`~/.pi/agent/sessions/--home-ndari-projects-cog--/2026-11-03T04-19-23-534Z_*.jsonl`), reused the cache fully after both its long answers (1,364 and 2,035 tokens).

**Findings (2026-11-03, from the wire log plus the server's `/apply-template` and `/tokenize`, no generation):**
- Psi's requests are right. The next request, rendered by the server's own template (with the server's `preserve_thinking: true`), starts with exactly the earlier prompt's tokens plus the answer's text re-tokenized, through to `<|im_end|>`. No think block or field goes missing.
- The model's own tokens differ from that re-tokenization. Each long answer took one token more than its text re-tokenizes to (1,900 against 1,899, 2,025 against 2,024), while every short answer, and all of pi's answers, match. So each long answer had one spot where the model wrote one usual token as two. The streamed chunks point to `Module/import` in the first and ` |>` in the second (code-like text). Most likely, not proven: that needs the raw token ids.
- Why one odd token costs the whole answer: the 35B is a hybrid model, so llama-server can't cut its cache back to the exact point of difference; it restores the last checkpoint, which here sits 4 tokens before the end of the previous prompt (the "4 tokens" seen). As far as I know from llama.cpp, checkpoints are taken only while reading a prompt, not during generation (not checked in its source); the server's checkpoint flags (`--ctx-checkpoints`, `--checkpoint-min-step`) offer nothing for generation.
- Pi isn't doing anything different (no sampling fields; its llama.cpp extension adds only thinking settings). Its answers happened to contain no such split. It's chance per answer, more likely in long and code-heavy answers.

**Outcome (2026-11-03):** accepted as is, option 1 below.

**Options, none built:**
1. Accept it: about 3.5 s once after some long answers, no correctness cost.
2. Server sampling: the model card's non-thinking settings (`--temp 0.7 --top-p 0.8`, the server uses temp 1.0 now) make rare token choices less likely. Fewer cases, not none. Overlaps with the "faster 35B" sampling candidate.
3. Psi re-reads the answer in the background: after a final answer, compare the answer's `/tokenize` count with the generated count, and on a mismatch send the history through the answer (raw prompt to `/completion`, no output) while the user reads, so the next turn finds a checkpoint near the end. llama-server only; needs a check that it doesn't slow a prompt the user sends straight away (`-np 1`).
4. Check whether Gemma shows it. Its sliding-window attention also limits how far llama-server can cut the cache back, so it may behave the same.

### Does Psi's prompt steer the model to code over docs? (added 2026-11-02)

**Question:** asked to review a project, why does the model in Psi read only source files while in pi it reads the README, docs and plans first?

**Context:** the same review in `../cog` in both harnesses, the 35B on the Mac, with llama-server restarted between them so no cache carried over (sessions: Psi `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-0334_408b21c4.jsonl`, pi `~/.pi/agent/sessions/--home-ndari-projects-cog--/2026-11-03T04-19-23-534Z_*.jsonl`).
- Psi: `find`, `wc -l`, `ls -la`, then 19 `.go` files read whole, then `go test`, `go vet`, `go test -race`. It never opened `README.md`, `docs/`, `plan/` or `examples/`, though `ls -la` listed them and cog's CLAUDE.md (in the system prompt) says the semantics are documented in README.md and docs/. Asked about the language itself, it answered with no tool calls and began "I've read through the language docs, semantics, stdlib", which it hadn't.
- Pi: `cat README.md` in its first step, listed `docs/`, `plan/` and `examples/`, read `plan/known-issues.md`, `plan/design-review.md` and `plan/semantic-foundation.md`, then a few source files (some with `limit`). For the language question it read `docs/words.md`, `semantics.md`, `modules.md`, `dictionaries.md`, `metaprogramming.md`, `embedding.md` and the examples.

**Candidate causes in Psi's prompt:**
- "Understand before you change: read the relevant code first, and look for existing helpers, conventions and tests." It names code, never docs.
- Pi's prompt has a "Pi documentation" section ("read the docs and examples, and follow .md cross-references", "Always read pi .md files completely"). It's about pi's own docs, but it may prime the model to read `.md` files in general. Psi has nothing like it.
- The CLAUDE.md "Implementation map" lists source files; both harnesses load it (pi reads CLAUDE.md too), so it doesn't explain the difference alone.

**Confounds:** the prompts weren't word-for-word the same. Psi's first was "What are the biggest issues that you see with the way it is implemented?", which points at code; pi's was "What improvements would you suggest to it?". Psi's language question ("what do you think of the language itself?") didn't say to look, while pi's did ("now take a look at the language itself"). Pi's session also began with "hi", and ran at thinking level medium (no thinking blocks were recorded). One run each, so this is a lead, not a finding.

**How:**
1. Rerun both with identical prompts, pasted from one file, the server restarted before each, and compare which files each reads.
2. If Psi still skips the docs, change the line to name docs too (for example "read the relevant docs and code first: README, docs/, and the files the task touches") and rerun once.
3. Model-facing text, so an eval run after the change (D-32); check that it doesn't make the model read docs on small fix tasks where they don't help.

**Rerun 2026-11-03** (one run each, the 35B on the Mac, `-p` then `-c -p` in a fresh copy of cog at HEAD without the earlier `qwen-*.md` outputs; the server wasn't restarted, since it can't drop its cache without `--slot-save-path`, and the two system prompts share only their first few tokens). Prompts, identical in both: "Take a look at this project. What improvements would you suggest to it?", then "Now take a look at the language itself. What do you suggest to make it better?". Sessions: `~/.psi/sessions/-tmp-claude-1000--home-ndari-projects-psi-50906382-555d-433d-8d13-7de8a3abd078-scratchpad-cog-/2026-11-03-1329_40c98505.*`, `~/.pi/agent/sessions/--tmp-claude-1000--home-ndari-projects-psi-50906382-555d-433d-8d13-7de8a3abd078-scratchpad-cog--/2026-11-03T13-35-35-478Z_*.jsonl`.
- First prompt: Psi read `README.md`, `go.mod` and `plan/known-issues.md` before any source, then the `.go` files, later `plan/semantic-foundation.md` and two examples. Pi read 13 `.go` files, then `README.md`, and only listed `docs/` and `examples/`.
- Language prompt: both read most of `docs/`, the examples and `plan/known-issues.md` and `plan/design-review.md`. Psi also read the README again; it tried `docs/metaprog.md`, which is `plan/metaprog.md`.
- **Outcome:** with the same prompts, Psi read the docs earlier than pi did. The difference in the cog session came from the prompt wording, not Psi's system prompt. No prompt change.

**Markdown in the answers** (owner noticed pi's looked better): neither system prompt says anything about formatting, and both models used headings, bold, code blocks and tables. Pi's answers used more of them (`###` per item, more tables), which is variance between runs. The rendering differs too: the same answer (pi's second one) opened in each TUI at 110 columns:
- Psi prints no blank lines between blocks; pi keeps one between paragraphs, headings, code blocks and rules. This is the biggest difference. `MdStream` drops blank lines and the UI commits blocks with nothing between them, and §11.5 doesn't ask for that.
- A `---` rule: Psi prints `---` dim, pi draws a line across.
- Headings: Psi shows the `##` dim before bold text (§11.5 "heading `#`s dim"), pi shows the text only, styled.
- Tables: Psi draws aligned columns with a line under the header; pi draws a full box with a line between rows. Psi's is more compact.
- Code blocks: both keep the fence lines; pi indents the block.

**Open:**
- The made-up "I've read the docs" claim is a separate model habit; a prompt line won't stop it, and catching it in the harness (a claim of reading files that no tool call read) is a bigger idea.

### Record the model in the session (added 2026-11-02)

**Question:** should a session record which model it ran with? Every entry and request now says `"model":"local"` (the llama-server alias), so a session file can't tell the 35B from Gemma afterwards.

**Context:** Psi already reads `model_path` from `/props` for the status line (`crates/psi-ai/src/client.rs`), but doesn't write it to the session. The `../cog` session above only shows the 35B through the wire log's server reply and the Mac's `/props`. Pi has the same gap: its session's `model_change` entry says `"modelId":"local"`.

**How:** put the `/props` model file name (and maybe `n_ctx` and the server build, `system_fingerprint`) in `session_info`, and a new entry when the poll sees the server's model change mid-session. Not model-facing, so no eval run; a test with the replay server.

**Open:**
- Whether the per-message `model` should stay the alias or become the file name; what pi records there is unchecked.
- `psi inspect` and the eval reports could show it once it's recorded.

**Built 2026-11-03.** `session_info` carries `server`: the `/props` model file name, its full path, `n_ctx` and the build. A later run on the same session that finds a different server (a resumed session, a restarted server) records a `server` entry with the new values; the same server adds nothing. `psi inspect` prints `model <file>` under `cwd`. Requests still send the alias. Not covered: a model swapped while one run is going (Psi reads `/props` once at start), and the eval reports, which already print the model from `/props`.

### Clear more at once, so the cache breaks less often (added 2026-11-03)

**Question:** each clearing of old tool results breaks the prompt cache from the first cleared result, near the start of the conversation, so the server re-reads almost everything. Would freeing more per clearing (so it happens once or twice instead of every few steps) save most of that time without hurting the model?

**Context:** the owner found Psi slower than pi on the 35B. In the docs-steer rerun (64K, `~/.psi/sessions/-tmp-claude-1000--home-ndari-projects-psi-50906382-555d-433d-8d13-7de8a3abd078-scratchpad-cog-/2026-11-03-1329_40c98505.*`), clearing ran 4 times in 30 requests (at about 50K, each freeing about 20K, §7.4: down to 20% of the window under the 70% trigger). Each was followed by a re-read of 24K to 31K tokens from depth 2.3K, 114 s of the 232 s the server spent reading. The context then refilled in a few steps of whole-file reads and cleared again. Pi compacted once in its run (it triggers at the window less 16K). Asking for prompt progress costs nothing: read speeds match with and without it at the same depth. One run each, so this shows the mechanism, not how much slower Psi is.

**How:** the cost of a clearing is the re-read after the first cleared result, almost independent of how much is cleared, so clear further down per batch: for example to 40% of the window (about 26K at 64K) instead of 20% under the trigger. Count the clearings and the re-read seconds on the same rerun, and check at 16K that the step budget and the re-read loop fixes still hold. It changes what the model sees, so an eval run (D-32), on the owner's go.

**Open:**
- Clearing more removes results the model may still need; the placeholders say how to get them back, and re-reading one file is cheaper than re-reading the context.

**Superseded 2026-11-05:** clearing is off by default (plan §7.4; "The cog review in Psi and pi, and clearing off" above).

**Line numbers cost a third more tokens (found 2026-11-03).** The owner noticed the slowness from the start too, before any clearing. Early steps reuse the cache fully, so the reading is the new tool output, and Psi's `read` numbers every line (§5, "line-numbered output"). Qwen's tokenizer splits numbers into single digits, so `1023\t` is 5 tokens before the code. The same seven files read in both runs (fn.go, env.go, parser.go, lexer.go, README.md, semantics.md, known-issues.md) took 31,559 tokens in Psi and 23,229 in pi, 35% more (20% to 49% by file). Pi's read has no line numbers. That slows every step that reads and fills the window sooner, which brings the clearing sooner. Built 2026-11-03: read shows lines without numbers, as pi does (offsets still come from the truncation notice; edit matches text, and still ignores numbers a model copies in). Plan §5 updated.

**Built and run 2026-11-03, both changes:** read without numbers, and clearing down to 30% of the window (§7.4). Quick set on the 35B, one run each, correctness only: 64K (`~/.psi/evals/2026-11-03-1457-quick-64k-noline-clear30`) 7 pass, 3 model failures, no harness failures; 16K (`2026-11-03-1507-quick-16k-noline-clear30`, `--n-ctx 16384 --max-output 4096`) 5 pass, 5 model failures, no harness failures. Every failure's transcript shows wrong logic in the model's code, and all four tasks behind them flip between pass and fail across earlier 35B runs. No sign the missing numbers misled the model: no `cat -n` or `nl`, `grep -n` twice (6 times before); 5 of 31 edits failed on misremembered text or a double match (0 of 37 in the 64K run before), each followed by the fix the error named. Neither 64K run cleared anything (the tasks don't fill 64K); at 16K summaries were already the usual path, as in the earlier 16K runs, and `multi_split_package` hit the step cap in its known loop. The cog review again (`.../scratchpad-cog-/2026-11-03-1520_acb65aa4.*`): 2 clearings in 25 requests, each freeing about 28K, against 4 in 30 before; re-reading after them took 32 s against 114 s.

### A faster 35B on the Mac for development (added 2026-11-01)

**Question:** how much faster (and cooler) can the 35B run on the M4 Max for day-to-day testing, quality aside? Where the time goes now (thinking-off quick set, 2026-11-01): 455 s generating at about 121 tok/s with MTP (draft acceptance 0.84 on average), 88 s reading prompts (about 1,000 tok/s on big ones). Generation is 84% of the model's time, so it is the lever; the prompt cache already keeps prefill small.

**Candidates, roughly in order of expected payoff:**
- The model card's non-thinking sampling (`--temp 0.7 --top-p 0.8 --top-k 20 --min-p 0 --presence-penalty 1.5`); the server's defaults are used now. Sharper sampling should also raise MTP's draft acceptance.
- `--spec-draft-n-max` 2, 3 (now), 4, 5. Unsloth says 2 suits most setups for this model; our acceptance (0.84 at 3) suggests longer drafts might pay.
- MTP plus n-gram drafting (`--spec-type draft-mtp,ngram-mod`): agent output often repeats code it just read.
- A smaller quant of the same model: UD-Q3_K_XL (17 GB against 23 GB for Q4); generation is bandwidth-bound, so fewer bits means faster tokens, at some quality cost.
- `-ub`/`-b` 1024 or 2048, for prefill only (16% of the time).
- KV cache `q8_0`: less memory traffic at depth; check quality.
- MLX (mlx-lm, oMLX) instead of llama.cpp: often reported faster on Apple Silicon for MoE, but MTP isn't in mlx-lm's main release, and Psi relies on llama-server (`/props`, its templates), so this would need Psi work first.

**How:** llama-bench can't measure MTP or sampling, so replay a fixed set of recorded requests (from an eval run's `wire.jsonl`) against llama-server for each setting and read its timing lines (tok/s, draft acceptance). One setting changed at a time, the Mac otherwise idle. A small script or `just` recipe; `llama-optimize` (github.com/bigattichouse/llama-optimize) automates such sweeps but is Linux-first with Metal untested.

**Other models:** none found that is clearly faster at a similar level (2026-11-01 search). The 35B-A3B is already the fast class: about 3B parameters active per token, and on a Mac speed follows the active size. Qwen3.6-27B (dense) is reported as more capable but much slower; Qwen3-Coder-30B-A3B is older with the same active size; Gemma 4 26B-A4B has more active parameters. Smaller dense models are slower than a 3B-active MoE.

**Heat:** total energy per task falls with fewer and faster tokens (thinking off already cut the quick set from about 15 to 9 minutes). macOS Low Power Mode lowers clocks: cooler, but slower.

**Run 2026-11-01** (replaying the 37 requests of four recorded trials in order, seed 0, output capped at 2048 tokens, one setting changed at a time; scripts were in the session scratchpad): generation tok/s, MTP draft acceptance.

| Setting | Gen tok/s | Acceptance |
|---|---|---|
| UD-Q3_K_XL (17 GB) | 138.4 | 0.79 |
| Baseline: UD-Q4_K_XL, `--spec-draft-n-max 3`, server sampling | 128.0, 127.4 (first and last) | 0.81 |
| `--spec-draft-n-max 5` | 127.1 | 0.73 |
| `--spec-draft-n-max 4` | 123.4 | 0.74 |
| MTP + `ngram-mod` | 119.4 | 0.60 |
| Card sampling | 119.0 (and 12% more tokens written) | 0.83 |
| `--spec-draft-n-max 2` | 118.1 | 0.86 |

The two baseline runs matched (identical output, 128.0 and 127.4), so heat didn't skew the order. The current setup is already at or near the best; only the smaller quant is faster, by about 8%, at an untested quality cost. Kept as is. Not tried: `-ub`/`-b` (prefill is a small share) and KV `q8_0`.

