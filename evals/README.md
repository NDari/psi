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
| `Containerfile` | The trial image (`localhost/psi-eval:latest`): Debian trixie, Python 3.13, git, make, gcc, ripgrep, Node.js and pi 1.4.0 |
| `code/<task>/prompt.md` | The request, as a developer would write it |
| `code/<task>/repo/` | The starting project |
| `code/<task>/reference/` | A correct solution's files, overlaid on `repo/` |
| `code/<task>/hidden/check.sh` | The grader; prints one `PASS <name>` or `FAIL <name>: <why>` line per check |
| `code/<task>/hidden/total` | How many check lines `check.sh` prints |
| `code/_lib/` | Helpers for the graders (`check`, `check_output`, `unittests`) |
| `code/SET` | The quick set: what `psi eval run` runs by default |
| `pi/step-cap.ts` | A pi extension that stops a trial after the step cap |

The 40 tasks were ported from Heimdall. Names start with the kind of
task: `fix_` (Python bug fixes), `feat_` (Python features), `sh_` (shell
work), `multi_` (multi-file changes).

## Commands

Run these from the repository root (or pass `--evals`). Results go to
`~/.psi/evals/<UTC time>-<label>/` unless `--results` is given.

```
psi eval check                     # every grader: reference passes, untouched repo fails
psi eval run --agent pi            # the quick set, one trial each
psi eval run --tasks all           # every task
psi eval run --tasks fix_env_parser,sh_logreport
psi eval diff RUN_A RUN_B          # two runs side by side
psi inspect RUN/TASK/trial-0       # one trial as a timeline
```

`psi eval run` needs podman and a llama-server already running on
`127.0.0.1:<port>` (default 8080) with `--jinja`. It does not start or
restart the server; the run records what `/props` reports (model path,
build, context window, default sampling) in `run.json`.

The reference models (D-29), as the baselines were run (fish or bash):

```
llama-server -m <Qwen_Qwen3.5-4B-Q4_K_M.gguf> --jinja --alias local -c 131072 -ngl 99 --port 8080 --reasoning off --seed 0 --temp 0.7 --top-p 0.8 --top-k 20 --min-p 0 --presence-penalty 1.5 --repeat-penalty 1.0
llama-server -m <gemma-4-12b-it-qat-q4_0.gguf> --jinja --alias local -c 131072 -ngl 99 --port 8080 --reasoning off --seed 0 --temp 1.0 --top-p 0.95 --top-k 64 --min-p 0
```

The sampling flags are each model card's recommendation; the agents send
no sampling fields, so the server's flags apply (H-08).

## What a trial does

1. **Setup.** The task's `repo/` is copied to `work/project/` inside a
   fresh git repository `work/`. The agent's home holds only its model
   config (pi: `~/.pi/agent/models.json`, provider `llamacpp`, model
   `local`, context window from `/props`, thinking off).
2. **The run.** A rootless podman container (`--userns=keep-id`) runs the
   agent in `/work/project` with a timeout (default 1200 s). Its only
   network route is the model server (`pasta:--splice-only,-T,<port>`); a
   check before the trials confirms the server is reachable and the
   internet and another host port are not (`network.txt`). pi runs as
   `pi --mode json -p <prompt> --model llamacpp/local --tools
   read,bash,edit,write -e /eval-pi/step-cap.ts`, offline.
3. **The record.** pi's event stream (`run.jsonl`) is converted into Psi
   session entries (`session.jsonl`), so inspection and scoring read every
   agent alike (D-31).
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

## Adding a task

Add `code/<name>/` with the files above and run
`psi eval check --tasks <name>`: the reference must pass every check and
the untouched repo must fail at least one. Keep the checks to what the
prompt states, so any correct solution passes.
