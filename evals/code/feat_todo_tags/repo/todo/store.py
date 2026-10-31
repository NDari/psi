"""Keeping tasks in a JSON file.

The file looks like {"version": 1, "tasks": [...]}. A missing file is an
empty list. Writes go to a temporary file that then replaces the old one,
so a crash never leaves half a file behind.
"""
import json
import os
import tempfile

from .model import Task, TodoError

VERSION = 1


def default_path():
    return os.environ.get("TODO_FILE") or "todo.json"


class Store:
    def __init__(self, path):
        self.path = path

    def load(self):
        try:
            with open(self.path, encoding="utf-8") as f:
                data = json.load(f)
        except FileNotFoundError:
            return []
        except ValueError as e:
            raise TodoError(f"{self.path}: not a todo file: {e}") from None
        if not isinstance(data, dict) or data.get("version") != VERSION:
            raise TodoError(f"{self.path}: unsupported file version")
        return [Task.from_dict(d) for d in data.get("tasks", [])]

    def save(self, tasks):
        data = {"version": VERSION, "tasks": [t.to_dict() for t in tasks]}
        folder = os.path.dirname(os.path.abspath(self.path))
        fd, tmp = tempfile.mkstemp(prefix=".todo-", dir=folder)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2)
                f.write("\n")
            os.replace(tmp, self.path)
        except BaseException:
            os.unlink(tmp)
            raise


def next_id(tasks):
    return max((t.id for t in tasks), default=0) + 1


def find(tasks, task_id):
    for t in tasks:
        if t.id == task_id:
            return t
    raise TodoError(f"no task {task_id}")
