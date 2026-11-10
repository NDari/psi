# Eval variants

Each directory here is copied into every psi trial's `~/.psi` with
`psi eval run --psi-config evals/variants/<name>` (psi trials only).

- `batteries`: a post-edit check that fits every quick-set task (unit
  tests for the Python ones, `bash -n` for the shell scripts). The trial
  records show what the check fed back.
