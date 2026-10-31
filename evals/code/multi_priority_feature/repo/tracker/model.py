"""The Task record and its validation rules.

A Task is validated when it is created and again whenever storage loads
it, so every other module can trust its fields.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from datetime import date

STATUSES = ("open", "done")
TITLE_MAX = 200
NOTES_MAX = 2000

_TAG_RE = re.compile(r"^[a-z0-9][a-z0-9_-]*$")
_PROJECT_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")


def normalize_tag(tag: str) -> str:
    """Lowercase a tag and drop one leading '#'. Raises ValueError when the
    result is not a valid tag."""
    tag = tag.strip().lower()
    if tag.startswith("#"):
        tag = tag[1:]
    if not _TAG_RE.match(tag):
        raise ValueError(f"invalid tag: {tag!r}")
    return tag


def normalize_tags(tags) -> list[str]:
    """Normalize a list of tags, dropping duplicates but keeping order."""
    seen: list[str] = []
    for tag in tags:
        t = normalize_tag(tag)
        if t not in seen:
            seen.append(t)
    return seen


def _parse_iso(value, name: str) -> date | None:
    if value is None or value == "":
        return None
    if isinstance(value, date):
        return value
    try:
        return date.fromisoformat(value)
    except (TypeError, ValueError):
        raise ValueError(f"{name}: not a date: {value!r}") from None


@dataclass
class Task:
    id: int
    title: str
    project: str | None = None
    tags: list[str] = field(default_factory=list)
    status: str = "open"
    created: date | None = None
    due: date | None = None
    done_on: date | None = None
    notes: str = ""

    def __post_init__(self):
        validate(self)

    # State

    @property
    def is_open(self) -> bool:
        return self.status == "open"

    @property
    def is_done(self) -> bool:
        return self.status == "done"

    def is_overdue(self, today: date) -> bool:
        return self.is_open and self.due is not None and self.due < today

    def is_due_on(self, day: date) -> bool:
        return self.is_open and self.due == day

    def mark_done(self, today: date) -> None:
        if self.is_done:
            raise ValueError(f"task {self.id} is already done")
        self.status = "done"
        self.done_on = today

    def reopen(self) -> None:
        if self.is_open:
            raise ValueError(f"task {self.id} is not done")
        self.status = "open"
        self.done_on = None

    # Serialization

    def to_dict(self) -> dict:
        return {
            "id": self.id,
            "title": self.title,
            "project": self.project,
            "tags": list(self.tags),
            "status": self.status,
            "created": self.created.isoformat() if self.created else None,
            "due": self.due.isoformat() if self.due else None,
            "done_on": self.done_on.isoformat() if self.done_on else None,
            "notes": self.notes,
        }

    @classmethod
    def from_dict(cls, data: dict) -> "Task":
        """Build a Task from a stored dict. Unknown keys are ignored and
        optional keys may be missing."""
        if not isinstance(data, dict):
            raise ValueError(f"task entry is not an object: {data!r}")
        if "id" not in data or "title" not in data:
            raise ValueError("task entry needs an id and a title")
        return cls(
            id=data["id"],
            title=data["title"],
            project=data.get("project"),
            tags=list(data.get("tags") or []),
            status=data.get("status", "open"),
            created=_parse_iso(data.get("created"), "created"),
            due=_parse_iso(data.get("due"), "due"),
            done_on=_parse_iso(data.get("done_on"), "done_on"),
            notes=data.get("notes") or "",
        )


def validate(task: Task) -> None:
    """Check every field of task, normalizing tags and the title in place.
    Raises ValueError with a message naming the field."""
    if isinstance(task.id, bool) or not isinstance(task.id, int) or task.id < 1:
        raise ValueError(f"id: must be a positive integer, got {task.id!r}")
    if not isinstance(task.title, str):
        raise ValueError("title: must be a string")
    task.title = " ".join(task.title.split())
    if not task.title:
        raise ValueError("title: must not be empty")
    if len(task.title) > TITLE_MAX:
        raise ValueError(f"title: longer than {TITLE_MAX} characters")
    if task.project is not None:
        if not isinstance(task.project, str) or not _PROJECT_RE.match(task.project):
            raise ValueError(f"project: invalid name {task.project!r}")
    task.tags = normalize_tags(task.tags)
    if task.status not in STATUSES:
        raise ValueError(f"status: must be one of {', '.join(STATUSES)}, got {task.status!r}")
    for name in ("created", "due", "done_on"):
        value = getattr(task, name)
        if value is not None and not isinstance(value, date):
            raise ValueError(f"{name}: must be a date")
    if task.status == "open" and task.done_on is not None:
        raise ValueError("done_on: set on an open task")
    if not isinstance(task.notes, str):
        raise ValueError("notes: must be a string")
    if len(task.notes) > NOTES_MAX:
        raise ValueError(f"notes: longer than {NOTES_MAX} characters")
