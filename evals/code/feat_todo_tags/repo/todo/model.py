"""The Task record and its JSON form."""
from dataclasses import dataclass, field

PRIORITIES = (1, 2, 3)  # 1 is most urgent


class TodoError(Exception):
    """A user-facing error; the CLI prints it and exits with status 1."""


@dataclass
class Task:
    id: int
    title: str
    done: bool = False
    priority: int = 2
    created: str = ""  # ISO date
    due: str | None = None  # ISO date or None

    def __post_init__(self):
        self.title = self.title.strip()
        if not self.title:
            raise TodoError("title must not be empty")
        if self.priority not in PRIORITIES:
            raise TodoError(f"priority must be one of 1, 2, 3, got {self.priority}")

    def to_dict(self):
        d = {
            "id": self.id,
            "title": self.title,
            "done": self.done,
            "priority": self.priority,
            "created": self.created,
        }
        if self.due is not None:
            d["due"] = self.due
        return d

    @classmethod
    def from_dict(cls, d):
        """Build a Task from its stored form. Unknown keys are ignored."""
        return cls(
            id=int(d["id"]),
            title=d["title"],
            done=bool(d.get("done", False)),
            priority=int(d.get("priority", 2)),
            created=d.get("created", ""),
            due=d.get("due"),
        )
