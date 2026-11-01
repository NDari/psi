# Recorded model mistakes

Model responses copied byte for byte from eval trials' wire logs, replayed
against today's Psi by `tests/recorded.rs`. They test the harness on what
models actually sent, not on what a test author imagines.

| Fixture | Model | Trial (`~/.psi/evals/...`) | Steps | What it shows |
|---|---|---|---|---|
| `qwen-blocked-retries` | Qwen 3.5 4B | `2026-11-01-1046-phase6-local-qwen3.5-4b/sh_changelog` | 15-22 | Blocked from a hard reset, tries other forms and `git clean` |
| `qwen-doubled-path` | Qwen 3.5 4B | `2026-10-31-1904-phase3-qwen3.5-4b/feat_logstats_jsonl` | 1 | A path that repeats the working directory, next to a good call |
| `gemma-loop` | Gemma 4 12B QAT | `2026-10-31-1904-phase3-gemma4-12b-qat/sh_changelog` | 11-22 | The same command again and again |

To add one: find the steps with `psi inspect <trial>` (its "patterns"
section flags loops, retries after a block, doubled paths, cut-off calls),
then `just record-fixture <trial> <steps> <name>` and write a test in
`tests/recorded.rs`. The trials ran in `/work/project`, and tool arguments
stream a token at a time, so paths can't be rewritten: the tests' workspace
ends in `work/project` instead. Responses cut off by the output limit are
about 2 MB each; leave those out.
