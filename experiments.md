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

## Done

(none yet)
