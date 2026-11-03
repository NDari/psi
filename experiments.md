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

### Prompt cache lost after a long final answer (added 2026-11-02)

**Question:** why does the next turn re-read a long final answer instead of reusing it from the prompt cache, and can Psi avoid it?

**Context:** in a hands-on session in `../cog` (the 35B on the Mac, `~/.psi/sessions/-home-ndari-projects-cog-/2026-11-03-0334_408b21c4.*`), two long answers that ended a turn (about 1,900 and 2,000 tokens) were each reprocessed whole on the next user prompt: `cache_n` fell about 1,905 and 2,030 tokens short of the previous request's prompt plus output. That put first token at about 3.5 to 4 s against about 0.3 s. Short final answers ("Done - ...", about 50 tokens) and assistant messages with tool calls were reused fully. The text Psi sent back is byte-identical to what was streamed. Pi, against the same server (`~/.pi/agent/sessions/--home-ndari-projects-cog--/2026-11-03T04-19-23-534Z_*.jsonl`), reused the cache fully after both its long answers (1,364 and 2,035 tokens).

**Findings (2026-11-03, from the wire log plus the server's `/apply-template` and `/tokenize`, no generation):**
- Psi's requests are right. The next request, rendered by the server's own template (with the server's `preserve_thinking: true`), starts with exactly the earlier prompt's tokens plus the answer's text re-tokenized, through to `<|im_end|>`. No think block or field goes missing.
- The model's own tokens differ from that re-tokenization. Each long answer took one token more than its text re-tokenizes to (1,900 against 1,899, 2,025 against 2,024), while every short answer, and all of pi's answers, match. So each long answer had one spot where the model wrote one usual token as two. The streamed chunks point to `Module/import` in the first and ` |>` in the second (code-like text). Most likely, not proven: that needs the raw token ids.
- Why one odd token costs the whole answer: the 35B is a hybrid model, so llama-server can't cut its cache back to the exact point of difference; it restores the last checkpoint, which here sits 4 tokens before the end of the previous prompt (the "4 tokens" seen). As far as I know from llama.cpp, checkpoints are taken only while reading a prompt, not during generation (not checked in its source); the server's checkpoint flags (`--ctx-checkpoints`, `--checkpoint-min-step`) offer nothing for generation.
- Pi isn't doing anything different (no sampling fields; its llama.cpp extension adds only thinking settings). Its answers happened to contain no such split. It's chance per answer, more likely in long and code-heavy answers.

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

**Line numbers cost a third more tokens (found 2026-11-03).** The owner noticed the slowness from the start too, before any clearing. Early steps reuse the cache fully, so the reading is the new tool output, and Psi's `read` numbers every line (§5, "line-numbered output"). Qwen's tokenizer splits numbers into single digits, so `1023\t` is 5 tokens before the code. The same seven files read in both runs (fn.go, env.go, parser.go, lexer.go, README.md, semantics.md, known-issues.md) took 31,559 tokens in Psi and 23,229 in pi, 35% more (20% to 49% by file). Pi's read has no line numbers. That slows every step that reads and fills the window sooner, which brings the clearing sooner. Built 2026-11-03: read shows lines without numbers, as pi does (offsets still come from the truncation notice; edit matches text, and still ignores numbers a model copies in). Plan §5 updated.

**Built and run 2026-11-03, both changes:** read without numbers, and clearing down to 30% of the window (§7.4). Quick set on the 35B, one run each, correctness only: 64K (`~/.psi/evals/2026-11-03-1457-quick-64k-noline-clear30`) 7 pass, 3 model failures, no harness failures; 16K (`2026-11-03-1507-quick-16k-noline-clear30`, `--n-ctx 16384 --max-output 4096`) 5 pass, 5 model failures, no harness failures. Every failure's transcript shows wrong logic in the model's code, and all four tasks behind them flip between pass and fail across earlier 35B runs. No sign the missing numbers misled the model: no `cat -n` or `nl`, `grep -n` twice (6 times before); 5 of 31 edits failed on misremembered text or a double match (0 of 37 in the 64K run before), each followed by the fix the error named. Neither 64K run cleared anything (the tasks don't fill 64K); at 16K summaries were already the usual path, as in the earlier 16K runs, and `multi_split_package` hit the step cap in its known loop. The cog review again (`.../scratchpad-cog-/2026-11-03-1520_acb65aa4.*`): 2 clearings in 25 requests, each freeing about 28K, against 4 in 30 before; re-reading after them took 32 s against 114 s.

## Done

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

