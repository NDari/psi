"""Selecting and ordering tasks for the list and report commands."""

from __future__ import annotations

from datetime import date

from .model import Task, normalize_tag

SORT_KEYS = ("id", "due", "title", "created", "project")
NO_PROJECT = "(none)"


def filter_tasks(
    tasks,
    status: str | None = "open",
    project: str | None = None,
    tag: str | None = None,
    overdue: bool = False,
    today: date | None = None,
    text: str | None = None,
) -> list[Task]:
    """The tasks matching every given condition, in their original order.

    status is "open", "done", or None (or "all") for both. project matches
    exactly; tag is normalized first. overdue needs today. text is a
    case-insensitive substring of the title or the notes.
    """
    if overdue and today is None:
        raise ValueError("overdue filter needs today")
    wanted_tag = normalize_tag(tag) if tag else None
    needle = text.casefold() if text else None
    out = []
    for t in tasks:
        if status not in (None, "all") and t.status != status:
            continue
        if project is not None and t.project != project:
            continue
        if wanted_tag is not None and wanted_tag not in t.tags:
            continue
        if overdue and not t.is_overdue(today):
            continue
        if needle is not None and needle not in t.title.casefold() and needle not in t.notes.casefold():
            continue
        out.append(t)
    return out


def _sort_key(key: str):
    # Missing values sort last; ties always fall back to the id.
    if key == "id":
        return lambda t: t.id
    if key == "due":
        return lambda t: (t.due is None, t.due or date.max, t.id)
    if key == "created":
        return lambda t: (t.created is None, t.created or date.max, t.id)
    if key == "title":
        return lambda t: (t.title.casefold(), t.id)
    if key == "project":
        return lambda t: (t.project is None, (t.project or "").casefold(), t.id)
    raise ValueError(f"unknown sort key: {key!r}")


def sort_tasks(tasks, key: str = "id", reverse: bool = False) -> list[Task]:
    """A new list of tasks ordered by key (one of SORT_KEYS)."""
    return sorted(tasks, key=_sort_key(key), reverse=reverse)


def group_by_project(tasks) -> dict[str, list[Task]]:
    """Tasks grouped by project name, names sorted, tasks without a project
    under NO_PROJECT at the end."""
    groups: dict[str, list[Task]] = {}
    for t in tasks:
        groups.setdefault(t.project or NO_PROJECT, []).append(t)
    names = sorted((n for n in groups if n != NO_PROJECT), key=str.casefold)
    if NO_PROJECT in groups:
        names.append(NO_PROJECT)
    return {n: groups[n] for n in names}


def all_tags(tasks) -> dict[str, int]:
    """Each tag in use with the number of open tasks carrying it."""
    counts: dict[str, int] = {}
    for t in tasks:
        for tag in t.tags:
            counts.setdefault(tag, 0)
            if t.is_open:
                counts[tag] += 1
    return dict(sorted(counts.items()))


def done_between(tasks, first: date, last: date) -> list[Task]:
    """Done tasks whose done_on falls in [first, last]."""
    return [t for t in tasks if t.is_done and t.done_on is not None and first <= t.done_on <= last]
