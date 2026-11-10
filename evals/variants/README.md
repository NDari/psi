# Eval variants

Each directory here is copied into every psi trial's `~/.psi` with
`psi eval run --psi-config evals/variants/<name>` (psi trials only).

- `batteries`: a post-edit check that fits every quick-set task (unit
  tests for the Python ones, `bash -n` for the shell scripts). The trial
  records show what the check fed back.
- `no-tail` (H-07): compaction keeps only the last user message or step
  verbatim after a summary, instead of the default tail (10K since 2026-11-04,
  capped at half the room left). Run it at a
  small window (`--n-ctx 16384`) so trials compact.
- `free-summary` (H-11): compaction summaries in plain prose with the
  same content, instead of the fixed headings. Also at `--n-ctx 16384`.
