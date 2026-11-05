# Experiments to run

Ideas the owner wants to try, kept so they don't get lost. Each entry says what to find out, how, and what's still open. When an experiment is run, note the date, where the results are, and what came of it; if it leads to a change, the change goes through `psi-plan.md` as usual.

Not the hypothesis table (`psi-plan.md` §14): those are design bets the plan commits to testing. These are the owner's own questions, often about the machine or the models rather than Psi.

## Open

### llama-server flags per machine (added 2026-11-01)

**Question:** which llama.cpp settings give the best prompt and generation speed for each reference model on each machine (this Linux box, RTX 4070 Ti SUPER 16 GB; the Mac, M4 Max 64 GB)?

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

### Speculative decoding for Gemma (added 2026-11-01)

**Question:** can Gemma 4 12B generate faster with speculative decoding, the way Qwen does with MTP (about 1.8x)? Gemma's GGUF has no MTP layers, so `llm-up gemma` runs with `--spec-type none`: about 58 tok/s generation and 2,500 tok/s prompt on the 4070 Ti SUPER (2026-11-01 eval run).

**How:** two kinds to try with llama-server, each on a short eval run or a few long answers, reading the server's timing lines:
- A small draft model with the same tokenizer: `--spec-type draft-simple --hf-repo-draft <small Gemma 4 GGUF>`. Which small Gemma 4 works as a draft is unchecked.
- No draft model: the n-gram types (`ngram-simple`, `ngram-mod`, `ngram-cache`, ...), which guess from text already in the context. They may suit agent work, where the model often rewrites code it just read.

**Open:**
- A draft model costs VRAM; check it still fits at 128K context on 16 GB.
- Speculative decoding shouldn't change the output, but confirm on one run that nothing looks off.

### A lower step cap for eval runs (added 2026-11-01)

**Question:** can eval runs be faster, and fail sooner, with a lower step cap? The owner asked about a cap of 10 (the default is 40).

**How:** a flat cap of 10 would turn most of the quick set into step-cap failures (35B passes took 6 to 40 steps; only `fix_invoice_rounding` and `sh_logreport` always finish within 10), which say nothing about the harness. The better candidate is a per-task cap of about twice the steps a pass takes (an optional `steps` file per task), as `just eval-ext` does with 20: a trial that hits it has gone well past normal and is worth reading.

**Open:**
- Earlier runs and the pi baselines used 40, so a changed cap breaks comparisons with them.
- Deferred by the owner until run time becomes a problem.

### write says what a replacement removed (added 2026-11-01)

**Question:** does the model keep code it didn't mean to drop if write's result names what the replacement removed? In `fix_retry_backoff` the 35B replaced `retry.py` with write and dropped the `retrying` decorator that `sync_job.py` imports, in both runs (`~/.psi/evals/2026-11-01-1638-screen-qwen3.6-35b`, `2026-11-01-1705-quick-g19-qwen3.6-35b`); the result only said "Replaced retry.py (29 lines)".

**How:** when write replaces a file, compare the definitions (`def`, `class`, `fn` and the like) in the old and new text and name the missing ones, for example "Replaced retry.py (38 → 29 lines). No longer defined: retrying. Put back anything removed by mistake." Then rerun `fix_retry_backoff` once.

**Open:**
- Model-facing text: a longer result on every replacement, and a nudge that may make the model restore code it meant to remove.
- Matching definitions across languages is a heuristic; a rename shows up as a removal.
- The owner isn't sure it's worth it yet.

### Sub-agent answers survive compaction (added 2026-11-01)

**Question:** at a small window, does a scout's report help the main agent if the harness keeps it through compactions?

**Context:** in the 16K rerun of `long_api_docs` with sub-agents on (`~/.psi/evals/2026-11-01-2045-h15-16k-fix-qwen3.6-35b`), the step budget's hint made the 35B call scout twice. Each scout read all 8 source files in its own context and returned a complete list of public functions (about 7,500 characters). The main agent then re-read files "to make sure", and two compactions later the report had been squeezed into a 230-word summary (n_ctx/70 words) that no longer held the function lists. It never wrote `docs/API.md`, and the compaction backstop paused it at step 33. Delegation works, but its result is lost before it pays off. At 64K this doesn't come up: the task passes without sub-agents.

**How:** put the latest one or two sub-agent answers into the harness state block written after the summaries (psi-plan.md §7.4 stage 3), taken as of the cut like the rest of the block so the prefix cache holds, with a size cap (say a tenth of the window, cut with a note). Then rerun `long_api_docs` at 16K (`--max-output 4096`, `subagents` on) once.

**Open:**
- Costs context on every request after a compaction; the cap decides how much.
- Which answers to keep: the latest, or only those from read-only agents.
- The main agent may still re-read to check, whatever it is given; that is a model habit, not something this fixes.

### Nudge when re-reading lines whose earlier read is gone (added 2026-11-01)

**Question:** does a one-line note stop the model re-reading a file that can't fit in the window, and get it to act on one part at a time?

**Context:** `multi_split_package` asks to split an 840-line `textkit.py` (about 8K tokens) into a package. At 16K the model wants the whole file in context before writing anything: in the rerun above it read it in 200-line pieces from top to bottom eight times, each piece pushing the previous one out, and never wrote a submodule (paused by the backstop at step 31; the 2026-11-01 16K runs before the fixes looped to the step cap the same way). The step budget can't help, since each step reads one piece. psi-plan.md §7.4 stage 4 already lists this as a candidate guard. `long_api_docs` loops the same way at 16K (2026-11-02): it reads all nine files whole, the step budget holds some back, and it reads them again until the backstop pauses it. It passed once before the step budget existed (`2026-11-01-1557`, a summary absorbed the files it hadn't read yet) and failed in all five 16K runs since.

**How:** count, per file, how often a read covers lines whose earlier read was cleared or summarized away. On the third time, add a line to the result, for example: "You have read these lines of textkit.py 3 times; the whole file can't fit in your 16384-token context. Write what this part needs now (or move lines with bash, e.g. `sed -n`), then read the next part." Rerun `multi_split_package` at 16K once.

**Open:**
- Model-facing text, and a nudge the 35B may ignore (the no-progress nudge fired twice without effect in the first 16K runs).
- Line ranges overlap loosely (the model picks different offsets each pass), so "the same lines" needs a tolerant match.
- Splitting a file without reading it all is hard for a model; this may only move the failure.

### MCP in the eval, and language-server tools (added 2026-11-01)

**Question:** does the 35B use MCP tools correctly in real runs, and do language-server tools (definition, references, diagnostics, rename) help on multi-file changes? This is H-16 in `psi-plan.md`; no eval run has used an MCP server yet, so this is the first MCP eval to do.

**Context:** two hands-on tries in the Psi repo (rust-analyzer, the 35B, "where is fit_context called" and "what breaks if plan_clear changes"):
- `isaacphi/mcp-language-server` v0.1.1 killed its own rust-analyzer half a second after starting (it sent a malformed message), so every call failed with "broken pipe" while the server stayed connected. That led to showing failed calls and stderr in `/mcp` and `psi mcp list`.
- The STRd6 fork (`go install github.com/STRd6/mcp-language-server@latest`, 19 tools, trimmed to definition, references, diagnostics, hover and rename_symbol: about 900 tokens) worked. The model called `references` first on its own both times. Rough spots: it guessed qualified names (`Agent::fit_context`, `Agent.plan_clear`) three times before the bare name worked; one `references` answer missed the production caller (`fit.rs:157`), probably rust-analyzer still indexing; results are positions only, so it read the files after; for callers of callers it went back to grep. On "where is X used" grep is already good, so the gain was small; diagnostics after an edit and cross-file renames are where it should matter more.

**How:**
1. Put a server in the eval image: pyright (`npm i -g pyright`) and the bridge binary built into the Containerfile, since the tasks are mostly Python and the container has no network. The `--psi-config` directory then holds an `mcp.json` (the bridge with `--lsp pyright-langserver -- --stdio`, `tools` trimmed, `approval: never` for the reading tools).
2. One quick-set run with it, read the transcripts: are the tools called, with correct names and arguments, and do failures stay readable? This is the correctness check for MCP itself.
3. Then the multi-file tasks (`multi_rename_callers`, `multi_config_object`, `multi_split_package`, `multi_shipping_bug`) with and without the server, one run each, directional only: steps, reads, and whether `rename_symbol` or `diagnostics` replace grep and test runs.

**Open:**
- The bridge is a third-party binary in the eval image; pin its version.
- The cold index may make the first calls incomplete; a warm-up (the first prompt waits up to 10 seconds for servers) may not cover indexing.
- The model guessing qualified symbol names is a model habit; a line in the tool description from the server can't be changed by Psi, but a skill or prompt line could say to use bare names.
- One run each; per AGENTS.md, a difference here is a direction, not a score.

**Run 2026-11-01** (`~/.psi/evals/2026-11-01-2215-h16-lsp-qwen3.6-35b`, the 35B at 64K, the quick set plus `multi_rename_callers` and `multi_shipping_bug`, variant `evals/variants/lsp`): 10 of 12 pass (8 of 10 on the quick set), no harness failures, and not one MCP call in 12 trials, though the five tools were declared on every request (about 900 tokens each time) and the server worked (checked in the image before the run; no errors in any trial). On `multi_rename_callers`, the task `rename_symbol` exists for, the model grepped, read the 8 files, made 11 edits and passed in 14 steps. So step 3 (with and without) has nothing to compare. The same pattern as sub-agents (H-15): the 35B uses an extra tool when the request points at it ("where is X called", in the hands-on tries) and not otherwise, and at 64K on these small projects grep and read are enough.

**Still open:** whether one system-prompt line naming the MCP tools and what they are for changes that (pi lists MCP servers in its system prompt); model-facing text, so a separate run. Projects big enough that grep misleads (many same-named symbols) would be the case where these tools should matter.

### The model card's sampling for the 35B (added 2026-11-03)

**Question:** do the model card's non-thinking settings (`--temp 0.7 --top-p 0.8 --top-k 20 --min-p 0 --presence-penalty 1.5`) beat the server's defaults (temp 1.0, top-p 0.95, min-p 0.05)? Sharper sampling should mean fewer odd token splits (the cache losses after long answers) and higher MTP draft acceptance (a server log line on 2026-11-03 showed 0.45, against 0.84 on average on 2026-11-01).

**How:** replay a fixed set of recorded requests against llama-server with each setting, as in the 2026-11-01 speed runs, and read generation tok/s and draft acceptance; count answers whose generated token count exceeds their re-tokenized length (the odd splits). Not pass rates: one run can't compare those.

**Open:**
- Presence penalty may hurt code (repeated identifiers); watch for it in the transcripts.

**Applied 2026-11-05 without the comparison:** `llm-up qwen35b` and `qwen27b` now pass the card's thinking-off settings. Before, the 35B ran on the sampling in its GGUF (`general.sampling.*`: temp 1.0, top-p 0.95, top-k 20, the card's thinking setting) and llama.cpp's defaults for the rest (min-p 0.05, presence penalty 0, which no GGUF key can set). The 27B had the thinking setting by flag.

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

## Done

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

**Still open:** `no_progress` should pause once a nudge goes unheeded and count exact repeats as no progress (`known-issues.md`); the sampling change alone didn't stop the loop with clearing on.

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

