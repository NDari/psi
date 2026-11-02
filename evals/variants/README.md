# Eval variants

Each directory here is copied into every psi trial's `~/.psi` with
`psi eval run --psi-config evals/variants/<name>` (psi trials only).

- `batteries`: the Phase 6 features a model can use on its own turned on:
  the todo tool, a post-edit check that fits every quick-set task (unit
  tests for the Python ones, `bash -n` for the shell scripts), and one
  skill about checking work. The trial records show whether the model
  calls todo, reads the skill, and what the check fed back.
- `lsp`: an MCP server (H-16): pyright through the `mcp-language-server`
  bridge (STRd6 fork, built into the image), offering definition,
  references, diagnostics, hover and rename_symbol, all without asking
  (headless trials deny calls that ask). The records show whether the
  model calls them, with what arguments, and what came back.
- `no-tail` (H-07): compaction keeps only the last user message or step
  verbatim after a summary, instead of the default tail (the larger of
  20% of the window and 20K, capped at half the room left). Run it at a
  small window (`--n-ctx 16384`) so trials compact.
- `free-summary` (H-11): compaction summaries in plain prose with the
  same content, instead of the fixed headings. Also at `--n-ctx 16384`.
