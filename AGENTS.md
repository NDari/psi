# Working on Psi

This file is for agents developing Psi itself. It is not what Psi's own models see: the prompts, tool descriptions and any AGENTS.md that Psi gives its models live in the harness and are designed separately.

## What Psi is

A pi-style coding agent harness in Rust + mlua, built for its owner as a daily replacement for pi, tuned for local models that make mistakes. `psi-plan.md` is the spec.

## Spirit of the project

- **Engineering over guarantees.** Prefer a simple, good-enough mechanism (a container as the boundary, checkpoints for undo) over a strict or exhaustive one. Strictness can be added later when there is a reason. Heimdall failed by starting strict and growing out of control; don't repeat that.
- **Small and working beats complete.** Build the least that makes a feature useful, then improve it from real use and eval runs.
- **The harness absorbs model mistakes.** Accept a call whose meaning is clear, refuse with the exact fix, or hard-gate. Don't build features that only work if a small model chooses to use them.
- **Keep model-facing text lean.** Every line of system prompt, tool description and tool result costs context. Tool results should say exactly what happened and what to do next.
- **Pi is the behavioral reference.** Read its source (`earendil-works/pi`, MIT) to match behavior, but write our own prompts, docs and code.
- **Heimdall is a source of ideas, not data.** Its lessons can inform a design; its numbers and timings are not baselines.

## How to work

- **The plan is the spec.** If a change contradicts `psi-plan.md`, stop and propose a plan edit first. Decisions are `D-xx` in §17; change one by adding a "Superseded" note with the reason.
- **Work phase by phase** (§15). Don't build ahead; leave `// TODO(phase-N): ...` stubs where later work plugs in.
- **Stay in scope.** Mention unrelated improvements; don't make them unasked.
- **Bring in dependencies when they earn it.** No need to ask: if a well-known, maintained crate provides value and saves writing a lot of code, add it. The owner wants a good user experience and a batteries-included feel. Appendix A lists candidates; check the current version and maintenance first, and mention what you added in your summary.
- **Don't run git write commands.** The owner commits.
- **No em-dashes or en-dashes** in code, comments, docs or messages. Use a hyphen or rephrase.

## Checks

Before calling anything done, run and report the result of:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If a check can't run, say what is unverified.

## Tests

- **Every behavior change gets a test.** Prefer scenario tests that drive the agent loop with a fake provider.
- **No test touches the network or a real model.** Live runs against llama-server and evals are manual.
- **Fixtures for model behavior come from real transcripts** where possible (tool-call shapes, loops, bad results), not invented ones.

## Evals

- Evals run locally against llama-server, each trial in a podman container. The container is the boundary; no inner sandbox.
- They are for sanity checks, finding harness bugs ("the tool result misled the model") and directional comparisons with pi. Don't chase small score differences or statistical significance unless asked.
- **Baselines are one run and prove correctness only.** A pi baseline or a phase's Psi score is a single run (one trial per task) showing the design works end to end. Never use one to judge speed, timing or how well an agent or model does, and don't re-run it for more trials. The owner considers this very important.
- When a run looks wrong, read the transcript and the wire log before changing anything.

## Rust conventions

- Stable toolchain, edition 2024, one cargo workspace with `psi-*` crates (§3.1).
- `thiserror` in library crates, `anyhow` in `psi-cli`. No `unwrap`/`expect` outside tests except for true invariants, with a comment saying why.
- `tracing` for logs; never print to stdout from library crates.
- Match the existing style of the file you're editing; don't reformat unrelated code.
