"""The Task record and its JSON form."""
import re
from dataclasses import dataclass, field

PRIORITIES = (1, 2, 3)  # 1 is most urgent
TAG = re.compile(r"[a-z0-9][a-z0-9-]*")


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
    tags: list = field(default_factory=list)  # lower case, in the order added

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
        d["tags"] = list(self.tags)
        return d

    def add_tags(self, names):
        for name in names:
            if name not in self.tags:
                self.tags.append(name)

    def remove_tags(self, names):
        self.tags = [t for t in self.tags if t not in names]

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
            tags=list(d.get("tags", [])),
        )


def normalize_tag(name):
    """Return the stored form of a tag name, or raise TodoError."""
    tag = name.lower()
    if not TAG.fullmatch(tag):
        raise TodoError(f"bad tag: {name}")
    return tag


def normalize_tags(names):
    return [normalize_tag(n) for n in names or []]
