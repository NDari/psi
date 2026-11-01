# Psi evals

The coding eval (psi-plan.md §12.5). It exists for sanity checks, for
finding harness bugs ("a tool result sent the model the wrong way") and
for directional comparisons with pi. It is not a benchmark.

**A baseline is one run, one trial per task.** It proves the design works
end to end: trials run, records are captured, graders produce outcomes,
harness failures surface. Never use one to judge speed or how well an
agent or model does, and don't re-run it for more trials.

## Layout

| Path | |
|---|---|
| `Containerfile` | The trial image (`localhost/psi-eval:latest`): Debian trixie, Python 3.13, git, make, gcc, ripgrep, Node.js and pi 1.4.0; pyright and an MCP language-server bridge for the `lsp` variant |
| `code/<task>/prompt.md` | The request, as a developer would write it |
| `code/<task>/repo/` | The starting project |
| `code/<task>/reference/` | A correct solution's files, overlaid on `repo/` |
| `code/<task>/hidden/check.sh` | The grader; prints one `PASS <name>` or `FAIL <name>: <why>` line per check |
| `code/<task>/hidden/total` | How many check lines `check.sh` prints |
| `code/_lib/` | Helpers for the graders (`check`, `check_output`, `unittests`) |
| `code/SET` | The quick set: what `psi eval run` runs by default |
| `pi/step-cap.ts` | A pi extension that stops a trial after the step cap |

The 40 coding tasks were ported from Heimdall. Names start with the kind of
task: `fix_` (Python bug fixes), `feat_` (Python features), `sh_` (shell
work), `multi_` (multi-file changes).

## The compaction task

`long_api_docs` (not in the quick set) is for compaction at a small window
(psi-plan.md §7.4): documenting 8 modules means reading about 20K tokens,
so at a 16K window the context is compacted several times. Its rules,
including a reviewer's name, a ticket and a version for the last line, are
in `NOTES.md`, which the model reads first: facts that only a summary (or a
second read) carries to the end. Run it against a server started with
`just ctx=16384 llm-up qwen35b`, with `--tasks long_api_docs --max-output
4096`, and read the trial's compaction entries to see whether the facts
survived each summary. `gen.py` generated the task.

## The extension tasks

`ext_*` (four, from Heimdall's extension eval; H-01) ask for a psi Lua
extension in the project's `.psi/extensions/<name>/`: a `tool_call` gate
(`ext_lockfile_guard`), a `tool_result` rewrite with a flag
(`ext_trim_output`), a new tool (`ext_todo_scan`) and a tool with session
state plus a command (`ext_bookmarks`). They are not in the quick set.
`just eval-ext` runs them with `docs/extensions.md` and `docs/api.md` in
the model's context (as `~/.psi/AGENTS.md`, built from `docs/` at run time,
so it can't drift), with a step cap of 20 (`just ext-steps=40 eval-ext`
for more): passes took 7 to 16 steps, and the runs are read for harness
problems rather than scored. Headless runs skip the untrusted project's
`.psi`, so the extension never loads into the session writing it.

Grading copies the extension without its own `*_test.lua` files, adds the
task's hidden tests and runs `psi ext test` (`exttests` in `_lib`), one
check per test case. Graders run `psi`, so `psi eval check` and the trials
mount the eval binary into the grading container (`just eval-check` passes
the static one).

## Commands

Run these from the repository root (or pass `--evals`). Results go to
`~/.psi/evals/<UTC time>-<label>/` unless `--results` is given.

```
psi eval check                     # every grader: reference passes, untouched repo fails
psi eval run                       # psi on the quick set, one trial each
psi eval run --agent pi            # the same with pi
psi eval run --tasks all           # every task
psi eval run --tasks fix_env_parser,sh_logreport
psi eval diff RUN_A RUN_B          # two runs side by side
psi inspect RUN/TASK/trial-0       # one trial as a timeline
```

Trials run a static (musl) build of psi, mounted into the container:
the image's glibc is older than this machine's, and psi's vendored Lua
ties a glibc build to the glibc it was built against. `just eval-run`
builds it (`just eval-build`, which needs the `musl` package and the
`x86_64-unknown-linux-musl` Rust target) and passes it with
`--psi-binary`; `psi eval run` checks first that the binary starts in
the image.

`psi eval run` needs podman and a llama-server already running on
`127.0.0.1:<port>` (default 9931) with `--jinja`. It does not start or
restart the server; the run records what `/props` reports (model path,
build, context window, default sampling) in `run.json`.

The reference models (D-29) start with `just llm-up qwen` or `just llm-up gemma` (`just llm-down` stops it): Qwen3.5 4B Q4_K_M and Gemma 4 12B QAT Q4_0, context 131072, thinking off, seed 0, on port 9931. The exact command goes to `.llm/<port>/cmd`. Two servers can run side by side on different ports, for example `just port=9932 llm-up qwen`, then `just port=9932 eval-run`. `just eval-check` and `just eval-run` wrap the commands above.

The sampling flags are each model card's recommendation; the agents send
no sampling fields, so the server's flags apply (H-08).

## What a trial does

1. **Setup.** The task's `repo/` is copied to `work/project/` inside a
   fresh git repository `work/`. The agent's home holds only its model
   config, with the context window from `/props`: psi gets
   `~/.psi/models.jsonc` (model `local`), pi gets
   `~/.pi/agent/models.json` (provider `llamacpp`, model `local`, thinking
   off).
2. **The run.** A rootless podman container (`--userns=keep-id`) runs the
   agent in `/work/project` with a timeout (default 1200 s). Its only
   network route is the model server (`pasta:--splice-only,-T,<port>`); a
   check before the trials confirms the server is reachable and the
   internet and another host port are not (`network.txt`). psi runs as
   `psi -p <prompt> --json --max-turns <steps> --isolation off` (the container is the boundary; the binary running
   `psi eval`, mounted read-only); pi runs as `pi --mode json -p <prompt>
   --model llamacpp/local --tools read,bash,edit,write -e
   /eval-pi/step-cap.ts`, offline.
3. **The record.** psi's own session file and wire log are copied out
   (`session.jsonl`, `wire.jsonl`); pi's event stream (`run.jsonl`) is
   converted into the same session entries, so inspection and scoring read
   every agent alike (D-31).
4. **Grading.** The project as the agent left it (without `.git`) is
   copied into a fresh container with no network and graded by
   `hidden/check.sh` (`grade/checks.txt`).

## Outcomes

- **PASS:** every check passed.
- **MODEL-FAIL:** the agent ran normally but some check failed, including
  a timeout or the step cap.
- **HARNESS-FAIL:** the trial could not be judged: the container failed,
  the agent crashed or exited non-zero, gave no final event, a model
  request failed (other than the step cap's abort), or grading failed.
  These never count as model results; read the trial's `run.stderr`.

Per trial, `<run>/<task>/trial-0/` holds `result.json`, `run.jsonl`,
`run.stderr`, `session.jsonl`, `grade/` and the agent's `home/` and
`work/`. The run holds `run.json`, `network.txt` and `summary.md`.

## The smoke test

`just eval-smoke` (part of `just check`) runs `psi eval run` for both
agents on fix_invoice_rounding with the replay server as the model,
scripted to write the reference solution. It checks the whole pipeline
(podman, network check, agent, record, grading) in a few seconds, without
a GPU.

## Adding a task

Add `code/<name>/` with the files above and run
`psi eval check --tasks <name>`: the reference must pass every check and
the untouched repo must fail at least one. Keep the checks to what the
prompt states, so any correct solution passes.
