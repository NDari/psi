"""Turning tasks into text for the terminal."""

from __future__ import annotations

from datetime import date

from .dates import format_date, humanize
from .model import Task


def format_line(task: Task, today: date) -> str:
    """One line per task, as `list` prints it:

        ID [ ] TITLE (PROJECT) due YYYY-MM-DD #tag #tag

    The id is right-aligned in three columns, [x] marks a done task, the
    parts after the title appear only when set, and an open task past its
    due date gets "!" after the date.
    """
    box = "[x]" if task.is_done else "[ ]"
    parts = [f"{task.id:>3} {box} {task.title}"]
    if task.project:
        parts.append(f"({task.project})")
    if task.due:
        mark = "!" if task.is_overdue(today) else ""
        parts.append(f"due {task.due.isoformat()}{mark}")
    if task.tags:
        parts.append(" ".join("#" + t for t in task.tags))
    return " ".join(parts)


def format_detail(task: Task, today: date) -> str:
    """Every field of one task, for `show`."""
    rows = [
        ("Task", f"#{task.id}"),
        ("Title", task.title),
        ("Status", task.status),
        ("Project", task.project or "-"),
        ("Tags", " ".join("#" + t for t in task.tags) or "-"),
        ("Created", format_date(task.created)),
        ("Due", _due_text(task, today)),
    ]
    if task.is_done:
        rows.append(("Done", format_date(task.done_on)))
    if task.notes:
        rows.append(("Notes", task.notes))
    width = max(len(k) for k, _ in rows)
    lines = []
    for key, value in rows:
        first, *rest = value.splitlines() or [""]
        lines.append(f"{key + ':':<{width + 1}} {first}")
        for more in rest:
            lines.append(" " * (width + 2) + more)
    return "\n".join(lines)


def _due_text(task: Task, today: date) -> str:
    if task.due is None:
        return "-"
    return f"{task.due.isoformat()} ({humanize(task.due, today)})"


def format_table(headers, rows) -> str:
    """A plain table: columns left-aligned except numbers, which are
    right-aligned; two spaces between columns."""
    table = [list(map(str, headers))] + [[str(c) for c in r] for r in rows]
    widths = [max(len(r[i]) for r in table) for i in range(len(headers))]
    numeric = [all(_is_number(r[i]) for r in table[1:]) and len(table) > 1 for i in range(len(headers))]
    out = []
    for r in table:
        cells = []
        for i, c in enumerate(r):
            cells.append(c.rjust(widths[i]) if numeric[i] else c.ljust(widths[i]))
        out.append("  ".join(cells).rstrip())
    return "\n".join(out)


def _is_number(s: str) -> bool:
    return s.isdigit()
