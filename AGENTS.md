# Working on Psi

This file is for agents developing Psi itself. It is not what Psi's own models see: the system prompt and tool descriptions that Psi gives its models live in the harness and are designed separately.

## What Psi is

One person's coding agent for a local model, written in Rust. It runs one small task at a time, in a fresh context, inside a boundary where nothing unrecoverable can happen, and ends each task with a report the harness wrote.

| Document | What it holds |
|---|---|
| `design-going-forward.md` | What Psi is for, the rules, what was cut and why. Read it first; it wins over everything else. |
| `psi-plan.md` | A short reference to how the parts work. Code comments cite its sections (`§8.4`) and decisions (`D-38`). |
| `experiments.md` | Questions put to a model, how each was measured and what came out. |
| `known-issues.md` | Defects we know about and chose not to fix yet, with how much they matter. |
| `docs/help/` | The help topics built into the binary (`psi help <topic>`). |
| `evals/README.md` | The eval runner and how to try a new model. |
| `docs/history/` | Dated documents that no longer describe the code. Read for background; never follow. |

## Rules of the design

These come from `design-going-forward.md`; a change that goes against one needs that file changed first.

- **Refuse, never ask.** Nothing waits for an answer. An action can be undone, or it is refused with what to do instead.
- **Limit what can be reached, don't classify what is said.** The sandbox, hidden credentials and checkpoints do the work; the refuse list stays short and is not grown.
- **The harness reports; the model is not asked to.** Checks are run by the harness. The model's claims are not evidence.
- **No feature that works only if the model chooses it.** Accept a call whose meaning is clear, refuse with the exact fix, or stop the run.
- **A run is bounded.** Requests, tool calls, summaries and repeats each have a limit, and at a limit the run stops with the reason.
- **One class of model, one user.** Tune for what runs on the owner's hardware now. Nothing is built for strangers.
- **Work around first.** A harness change needs the problem seen more than once, on ordinary tasks. One failed trial is not a reason.

## Spirit of the project

- **Engineering over guarantees.** Prefer a simple, good-enough mechanism over a strict or exhaustive one. Heimdall, Psi's predecessor, failed by starting strict and growing out of control.
- **Small and working beats complete.** Build the least that makes a feature useful, then improve it from real use.
- **Keep model-facing text lean.** Every line of system prompt, tool description and tool result costs context. Tool results say exactly what happened and what to do next.
- **Parity with any other agent is not a goal.** Don't add or keep a feature because pi or another agent has it.

## How to work

- **Stay in scope.** Mention unrelated improvements; don't make them unasked.
- **Try model-facing text before building it.** `psi replay` sends a recorded request again with an edit and tallies what the model does next. New or changed model-facing text needs that evidence and the owner's yes.
- **Never run a model without the owner's go.** That covers `psi eval run`, `psi replay` and anything else that reaches llama-server. The server has one slot, so one run at a time.
- **Write findings where they belong:** a model run and its result in `experiments.md`, a defect left for later in `known-issues.md`, a finished piece of the design under "Done" in `design-going-forward.md`. Don't create other planning or analysis documents unless asked.
- **Bring in dependencies when they earn it.** If a well-known, maintained crate saves a lot of code, add it, check its current version and maintenance first, and mention it in your summary.
- **Don't run git write commands** unless the owner asks in that message. The owner commits.
- **No em-dashes or en-dashes** in code, comments, docs or messages. Use a hyphen or rephrase.

## Checks

Before calling anything done, run `just check` and report the result. There is no hosted CI. It runs:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
just eval-smoke        # the eval pipeline through podman, with the replay server as the model (Linux only)
```

`just` lists the other recipes. If a check can't run, say what is unverified. The Seatbelt tests run only on macOS; the owner says when to run them there.

## Tests

- **Every behavior change gets a test.** Prefer scenario tests that drive the agent loop with the replay server (`psi_ai::replay`), playing back recorded llama-server responses (`crates/psi-ai/tests/fixtures/llama-server`) or a recorded transcript.
- **No test touches the network or a real model.**
- **Fixtures for model behavior come from real transcripts** where possible (tool-call shapes, loops, bad results), not invented ones.
- **Help topics are tested too:** each file in `docs/help/` is embedded and may be 30 lines at most.

## Evals

- Evals run against llama-server, each trial in a podman container. The container is the boundary; Psi's own sandbox is off inside it.
- They are for sanity checks, finding harness bugs ("the tool result misled the model") and trying a new model. Don't chase small score differences.
- **A run is one trial per task and proves correctness only.** It shows the pipeline works end to end and what goes wrong. Never use one to judge speed, timing or how well a model does, and don't re-run it for more trials. The owner considers this very important.
- **Run the eval sparingly:** when model-facing text changes, when a model changes, or to refresh replay fixtures. Everything else is covered by tests.
- When a run looks wrong, read the transcript and the wire log (`psi inspect`, `psi guards`) before changing anything.

## Rust conventions

- Stable toolchain, edition 2024, one cargo workspace with `psi-*` crates (`psi-plan.md` §3.1).
- `thiserror` in library crates, `anyhow` in `psi-cli`. No `unwrap`/`expect` outside tests except for true invariants, with a comment saying why.
- `tracing` for logs; never print to stdout from library crates.
- Match the existing style of the file you're editing; don't reformat unrelated code.
