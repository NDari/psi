"""Reading and writing the task file.

The file is JSON: {"version": 2, "tasks": [ ... ]}. Version 1 files were a
bare list of task objects; they still load, and the next save rewrites them
in the current format.
"""

from __future__ import annotations

import json
import os
import tempfile
from pathlib import Path

from .model import Task

FORMAT_VERSION = 2
DEFAULT_NAME = ".tracker.json"


class StorageError(Exception):
    """The task file could not be read or written."""


def default_path() -> Path:
    """The task file: $TRACKER_FILE, or ~/.tracker.json."""
    env = os.environ.get("TRACKER_FILE")
    if env:
        return Path(env)
    return Path.home() / DEFAULT_NAME


def load(path) -> list[Task]:
    """Load every task from path. A missing or empty file has no tasks."""
    path = Path(path)
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return []
    except OSError as e:
        raise StorageError(f"cannot read {path}: {e.strerror}") from None
    if not text.strip():
        return []
    try:
        data = json.loads(text)
    except json.JSONDecodeError as e:
        raise StorageError(f"{path}: not valid JSON (line {e.lineno})") from None

    if isinstance(data, list):
        entries = data
    elif isinstance(data, dict):
        version = data.get("version", 1)
        if not isinstance(version, int) or version > FORMAT_VERSION:
            raise StorageError(f"{path}: unsupported format version {version!r}")
        entries = data.get("tasks", [])
        if not isinstance(entries, list):
            raise StorageError(f"{path}: 'tasks' is not a list")
    else:
        raise StorageError(f"{path}: unexpected top-level value")

    tasks: list[Task] = []
    seen: set[int] = set()
    for i, entry in enumerate(entries):
        try:
            task = Task.from_dict(entry)
        except ValueError as e:
            raise StorageError(f"{path}: task #{i + 1}: {e}") from None
        if task.id in seen:
            raise StorageError(f"{path}: duplicate task id {task.id}")
        seen.add(task.id)
        tasks.append(task)
    return tasks


def dumps(tasks) -> str:
    """The file contents for tasks, sorted by id."""
    ordered = sorted(tasks, key=lambda t: t.id)
    data = {"version": FORMAT_VERSION, "tasks": [t.to_dict() for t in ordered]}
    return json.dumps(data, indent=2, ensure_ascii=False) + "\n"


def save(path, tasks) -> None:
    """Write tasks to path atomically: a temporary file in the same
    directory is renamed over the old one."""
    path = Path(path)
    text = dumps(tasks)
    directory = path.parent if str(path.parent) else Path(".")
    try:
        directory.mkdir(parents=True, exist_ok=True)
        fd, tmp = tempfile.mkstemp(prefix=".tracker-", dir=directory)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as f:
                f.write(text)
            os.replace(tmp, path)
        except BaseException:
            try:
                os.unlink(tmp)
            except OSError:
                pass
            raise
    except OSError as e:
        raise StorageError(f"cannot write {path}: {e.strerror}") from None


def next_id(tasks) -> int:
    """The id for a new task: one more than the largest id in use."""
    return max((t.id for t in tasks), default=0) + 1


def find(tasks, task_id: int) -> Task:
    """The task with task_id. Raises KeyError when there is none."""
    for t in tasks:
        if t.id == task_id:
            return t
    raise KeyError(task_id)
