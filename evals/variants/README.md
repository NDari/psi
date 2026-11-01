# Eval variants

Each directory here is copied into every psi trial's `~/.psi` with
`psi eval run --psi-config evals/variants/<name>` (psi trials only).

- `batteries`: the Phase 6 features a model can use on its own turned on:
  the todo tool, a post-edit check that fits every quick-set task (unit
  tests for the Python ones, `bash -n` for the shell scripts), and one
  skill about checking work. The trial records show whether the model
  calls todo, reads the skill, and what the check fed back.
