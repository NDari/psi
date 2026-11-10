# Working on Psi

This file is for agents developing Psi itself. It is not what Psi's own models see: the prompts, tool descriptions and any AGENTS.md that Psi gives its models live in the harness and are designed separately.

## What Psi is

One person's coding agent for a local model, written in Rust. `design-going-forward.md` says what Psi is for, what stays, what goes and in what order; read it first. `psi-plan.md` is the old spec, kept as history: where the two disagree, `design-going-forward.md` wins.

## Spirit of the project

- **Engineering over guarantees.** Prefer a simple, good-enough mechanism (a container as the boundary, checkpoints for undo) over a strict or exhaustive one. Strictness can be added later when there is a reason. Heimdall failed by starting strict and growing out of control; don't repeat that.
- **Small and working beats complete.** Build the least that makes a feature useful, then improve it from real use and eval runs.
- **The harness absorbs model mistakes.** Accept a call whose meaning is clear, refuse with the exact fix, or hard-gate. Don't build features that only work if a small model chooses to use them.
- **Keep model-facing text lean.** Every line of system prompt, tool description and tool result costs context. Tool results should say exactly what happened and what to do next.
- **Parity with pi is not a goal.** Don't add or keep a feature because pi has it. Pi's source (`earendil-works/pi`, MIT) can still be read for ideas.
- **Heimdall is a source of ideas, not data.** Its lessons can inform a design; its numbers and timings are not baselines.

## How to work

- **`design-going-forward.md` is the direction.** If a change contradicts it, stop and propose an edit to it first. `D-xx` references in code and docs point to `psi-plan.md` §17, which is history now.
- **Work in the order it gives** ("Order of work"), one step at a time. Don't build ahead, and don't follow the phases in `psi-plan.md` §15.
- **Stay in scope.** Mention unrelated improvements; don't make them unasked.
- **Bring in dependencies when they earn it.** No need to ask: if a well-known, maintained crate provides value and saves writing a lot of code, add it. The owner wants a good user experience and a batteries-included feel. Appendix A lists candidates; check the current version and maintenance first, and mention what you added in your summary.
- **Don't run git write commands.** The owner commits.
- **No em-dashes or en-dashes** in code, comments, docs or messages. Use a hyphen or rephrase.

## Checks

Before calling anything done, run `just check` (the local check, D-33; there is no hosted CI) and report the result. It runs:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`just` lists the other recipes (evals, llama-server). Add a recipe when a command gets run often.

If a check can't run, say what is unverified.

## Tests

- **Every behavior change gets a test.** Prefer scenario tests that drive the agent loop with the replay server (`psi_ai::replay`), playing back recorded llama-server responses (`crates/psi-ai/tests/fixtures/llama-server`) or a recorded transcript (D-32).
- **No test touches the network or a real model.** Live runs against llama-server and evals are manual.
- **Fixtures for model behavior come from real transcripts** where possible (tool-call shapes, loops, bad results), not invented ones.

## Evals

- Evals run locally against llama-server, each trial in a podman container. The container is the boundary; no inner sandbox.
- They are for sanity checks, finding harness bugs ("the tool result misled the model") and trying a new model. Don't chase small score differences or statistical significance unless asked.
- **Baselines are one run and prove correctness only.** A pi baseline or a phase's Psi score is a single run (one trial per task) showing the design works end to end. Never use one to judge speed, timing or how well an agent or model does, and don't re-run it for more trials. The owner considers this very important.
- **Run the eval sparingly** (D-32): when model-facing text changes, when a model changes, at the end of a phase, or to refresh replay fixtures. Everything else is covered by tests.
- When a run looks wrong, read the transcript and the wire log before changing anything.

## Rust conventions

- Stable toolchain, edition 2024, one cargo workspace with `psi-*` crates (§3.1).
- `thiserror` in library crates, `anyhow` in `psi-cli`. No `unwrap`/`expect` outside tests except for true invariants, with a comment saying why.
- `tracing` for logs; never print to stdout from library crates.
- Match the existing style of the file you're editing; don't reformat unrelated code.
