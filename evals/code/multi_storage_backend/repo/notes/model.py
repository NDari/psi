"""The note record."""
import dataclasses


@dataclasses.dataclass(frozen=True)
class Note:
    title: str
    body: str = ""
    tags: tuple = ()
    updated: int = 0  # unix seconds of the last change

    def to_dict(self):
        return {"title": self.title, "body": self.body, "tags": list(self.tags), "updated": self.updated}

    @classmethod
    def from_dict(cls, d):
        return cls(d["title"], d.get("body", ""), tuple(d.get("tags", ())), d.get("updated", 0))
