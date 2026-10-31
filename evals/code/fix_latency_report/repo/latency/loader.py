"""Reading gateway access logs."""
from dataclasses import dataclass
from datetime import datetime


@dataclass(frozen=True)
class Record:
    time: datetime
    method: str
    path: str
    status: int
    ms: int


class Skipped:
    """Counts lines that could not be parsed."""

    def __init__(self):
        self.count = 0


def parse_line(line):
    """Parse one log line into a Record, or return None if it is malformed."""
    parts = line.split()
    if len(parts) != 5:
        return None
    stamp, method, path, status, ms = parts
    if not ms.endswith("ms") or not status.isdigit() or not ms[:-2].isdigit():
        return None
    try:
        when = datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    except ValueError:
        return None
    return Record(when, method.upper(), _clean_path(path), int(status), int(ms[:-2]))


def _clean_path(path):
    path = path.split("?", 1)[0]
    if len(path) > 1:
        path = path.rstrip("/")
    return path


def read_log(path, skipped=None):
    """Yield the Records of the log file at path, in file order."""
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            if not line.strip():
                continue
            record = parse_line(line)
            if record is None:
                if skipped is not None:
                    skipped.count += 1
                continue
            yield record
