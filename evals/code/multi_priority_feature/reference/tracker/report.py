"""The summary printed by `tracker report`.

Example:

    Tasks: 8 total, 5 open, 3 done
    Overdue: 1
    Due today: 2
    Priority: high 1, normal 3, low 1
    Due this week: 1
    By project:
      home: 3 open, 1 done
      work: 2 open, 2 done
    Done in last 7 days: 2
    Oldest open: #2 Fix bike (created 2026-01-03)
"""

from __future__ import annotations

from datetime import date, timedelta

from .dates import week_range
from .filters import done_between, group_by_project


def build_report(tasks, today: date) -> str:
    tasks = list(tasks)
    open_tasks = [t for t in tasks if t.is_open]
    done_tasks = [t for t in tasks if t.is_done]
    lines = [f"Tasks: {len(tasks)} total, {len(open_tasks)} open, {len(done_tasks)} done"]
    lines.append(f"Overdue: {sum(1 for t in open_tasks if t.is_overdue(today))}")
    lines.append(f"Due today: {sum(1 for t in open_tasks if t.is_due_on(today))}")
    counts = {p: sum(1 for t in open_tasks if t.priority == p) for p in ("high", "normal", "low")}
    lines.append(f"Priority: high {counts['high']}, normal {counts['normal']}, low {counts['low']}")
    first, last = week_range(today)
    lines.append(f"Due this week: {sum(1 for t in open_tasks if t.due and first <= t.due <= last)}")
    lines.extend(_project_lines(tasks))
    recent = done_between(tasks, today - timedelta(days=6), today)
    lines.append(f"Done in last 7 days: {len(recent)}")
    lines.append(_oldest_line(open_tasks))
    return "\n".join(lines)


def _project_lines(tasks) -> list[str]:
    groups = group_by_project(tasks)
    if not groups:
        return ["By project: none"]
    out = ["By project:"]
    for name, items in groups.items():
        n_open = sum(1 for t in items if t.is_open)
        n_done = len(items) - n_open
        out.append(f"  {name}: {n_open} open, {n_done} done")
    return out


def _oldest_line(open_tasks) -> str:
    dated = [t for t in open_tasks if t.created is not None]
    if not dated:
        return "Oldest open: none"
    oldest = min(dated, key=lambda t: (t.created, t.id))
    return f"Oldest open: #{oldest.id} {oldest.title} (created {oldest.created.isoformat()})"
