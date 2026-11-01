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

