# Psi evals

The coding eval (psi-plan.md §12.5). It exists for sanity checks, for
finding harness bugs ("a tool result sent the model the wrong way") and
for trying a new model. It is not a benchmark.

**A baseline is one run, one trial per task.** It proves the design works
end to end: trials run, records are captured, graders produce outcomes,
harness failures surface. Never use one to judge speed or how well an
agent or model does, and don't re-run it for more trials.

## Layout

| Path | |
|---|---|
| `Containerfile` | The trial image (`localhost/psi-eval:latest`): Debian trixie, Python 3.13, git, make, gcc, ripgrep, Go |
| `code/<task>/prompt.md` | The request, as a developer would write it |
| `code/<task>/repo/` | The starting project |
| `code/<task>/source` | In place of `repo/`: a local git repository and a ref to start from (a task from a real project) |
| `code/<task>/setup.sh` | Optional: run on the host in the trial's copy of the project before the trial (fetch what it needs; trials have no network) |
| `code/<task>/reference/` | A correct solution's files, overlaid on `repo/`; tasks from real projects have none |
| `code/<task>/hidden/check.sh` | The grader; prints one `PASS <name>` or `FAIL <name>: <why>` line per check |
| `code/<task>/hidden/total` | How many check lines `check.sh` prints |
| `code/_lib/` | Helpers for the graders (`check`, `check_output`, `unittests`) |
| `code/SET` | The quick set: what `psi eval run` runs by default |
| `code/TRIAL` | The trial set (`--tasks trial`): longer tasks, 120 requests each, for trying a model |

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

## Commands

Run these from the repository root (or pass `--evals`). Results go to
`~/.psi/evals/<UTC time>-<label>/` unless `--results` is given.

```
psi eval check                     # every grader: reference passes, untouched repo fails
psi eval run                       # psi on the quick set, one trial each
psi eval run --tasks trial --model qwen27b   # the trial set with a profile's sampling
psi eval run --tasks all           # every task
psi eval run --tasks fix_env_parser,sh_logreport
psi eval diff RUN_A RUN_B          # two runs side by side
psi inspect RUN/TASK/trial-0       # one trial as a timeline
```

Trials run a static (musl) build of psi, mounted into the container:
the image's glibc is older than this machine's. `just eval-run`
builds it (`just eval-build`, which needs the `musl` package and the
`x86_64-unknown-linux-musl` Rust target) and passes it with
`--psi-binary`; `psi eval run` checks first that the binary starts in
the image.

`psi eval run` needs podman and a llama-server already running on
`127.0.0.1:<port>` (default 9931) with `--jinja`. It does not start or
restart the server; the run records what `/props` reports (model path,
build, context window, default sampling) in `run.json`.

The reference models (D-29) start with `just llm-up qwen` or `just llm-up gemma` (`just llm-down` stops it): Qwen3.5 4B Q4_K_M and Gemma 4 12B QAT Q4_0, context 131072, thinking off, seed 0, on port 9931. The exact command goes to `.llm/<port>/cmd`. Two servers can run side by side on different ports, for example `just port=9932 llm-up qwen`, then `just port=9932 eval-run`. `just eval-check` and `just eval-run` wrap the commands above.

The sampling flags in the justfile are each model card's recommendation
and the server's defaults. A model profile's `sampling` goes out with
every request and wins over them; `psi eval run --model <id>` gives the
trials that profile's sampling and thinking setting, and `run.json`
records them. Without `--model` the trials send no sampling fields.

## Trying a new model

1. Add a profile to `~/.psi/models.jsonc` with the model card's sampling
   (`psi help models`), and start the server.
2. `psi eval run --tasks trial --model <id>`: the trial set, one trial
   each, 120 requests a trial. The results directory is named after the
   model.
3. Read how each trial ended (`summary.md`, then `psi inspect` on the ones
   that did not pass). `psi eval diff RUN_A RUN_B` puts it next to a run
   of the model in use. One run each: it shows what goes wrong, not which
   model is better by a task or two.
4. `psi guards <run directory>`: which trials today's guards would stop,
   and how early.
5. For a failure worth a closer look, `psi replay <trial> --at <n>
   --samples 10` shows what the model does at that point, and the edit
   flags (`--append`, `--replace`, `--drop-last`, `--set`) try a change
   there before anything in psi is changed.

The trial set has one real prompt on a real project (`cog_bare_words`)
and the tasks here with the most to read and change. It should grow more
real prompts of the kind used by hand, on more than one project.

## A task from a real project

The loops in daily use came from long prompts on real projects, which the
small tasks here don't bring out. Such a task is:

- `prompt.md`: the prompt as it was typed.
- `source`: one line, the path to a local git repository and a ref in it,
  for example `~/projects/cog psi-trial-bare-words`. Make a branch or tag
  for the task, so the start stays the same while the project moves on.
  The trial gets that ref's files (`git archive`); the repository itself
  is not touched, and the task works only on a machine that has it.
- `setup.sh`, if the project needs something a trial can't fetch: it runs
  on the host in the trial's copy, before the trial (`go mod vendor`).
- `hidden/check.sh` and `hidden/total`: a simple outcome check, such as
  "the tests pass and this input gives that output".
- The project's toolchain in `Containerfile`.

There is no `reference/`: `psi eval check` then checks only that the
untouched project fails the grader. Add the task to `code/TRIAL`.

## What a trial does

1. **Setup.** The task's `repo/` is copied to `work/project/` inside a
   fresh git repository `work/`. The agent's home holds only its model
   config, with the context window from `/props`:
   `~/.psi/models.jsonc` (model `local`).
2. **The run.** A rootless podman container (`--userns=keep-id`) runs the
   agent in `/work/project` with a timeout (default 1200 s). Its only
   network route is the model server (`pasta:--splice-only,-T,<port>`); a
   check before the trials confirms the server is reachable and the
   internet and another host port are not (`network.txt`). psi runs as
   `psi -p <prompt> --json --max-turns <steps> --isolation off` (the container is the boundary; the binary running
   `psi eval`, mounted read-only).
3. **The record.** psi's own session file and wire log are copied out
   (`session.jsonl`, `wire.jsonl`); they are the record inspection and
   scoring read (D-31).
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

`just eval-smoke` (part of `just check`) runs `psi eval run`
on fix_invoice_rounding with the replay server as the model,
scripted to write the reference solution. It checks the whole pipeline
(podman, network check, agent, record, grading) in a few seconds, without
a GPU.

## Adding a task

Add `code/<name>/` with the files above and run
`psi eval check --tasks <name>`: the reference must pass every check and
the untouched repo must fail at least one. Keep the checks to what the
prompt states, so any correct solution passes.
