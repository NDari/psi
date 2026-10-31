Please add a priority to tasks in this tracker (`python3 -m tracker`, see README.md). It has to go through every layer:

- `tracker/model.py`: the `Task` dataclass gets a `priority` field, one of `"low"`, `"normal"`, `"high"`, default `"normal"`. Any other value raises `ValueError`, like the other fields.
- Storage: every task in a saved file gets a `"priority"` key. Files written before this change have no such key; they must still load, with every task as `"normal"`.
- `tracker/filters.py`: `filter_tasks` takes a new keyword argument `priority=None` (when set, only tasks with exactly that priority), and `sort_tasks` accepts the key `"priority"`: high first, then normal, then low, ties broken by id ascending.
- CLI: `add --priority low|normal|high` (default normal; any other value is an error), `list --priority P` to show only that priority, and `list --sort priority`. The list line format does not change.
- `report`: right after the `Due today:` line, add a line `Priority: high H, normal N, low L` counting open tasks only.

Everything else must keep working as before. Tests are in tests/; run `python3 -m unittest`.
