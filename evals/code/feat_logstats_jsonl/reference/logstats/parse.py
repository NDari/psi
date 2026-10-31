"""Reading access logs.

Two formats are supported. The Common Log Format (CLF), as written by
Apache and nginx:

    10.0.0.1 - frank [10/Oct/2026:13:55:36 +0200] "GET /a.gif?x=1 HTTP/1.1" 200 2326

and JSON lines, one object per line:

    {"host": "10.0.0.1", "time": "2026-10-10T13:55:36+02:00", "method": "GET",
     "path": "/a.gif?x=1", "status": 200, "bytes": 2326}

Lines that cannot be parsed are counted as skipped, not fatal: real logs
contain junk. Blank lines are ignored and not counted.
"""
import json
import re
from datetime import datetime

from .entry import Entry

CLF = re.compile(
    r'^(?P<host>\S+) \S+ \S+ \[(?P<time>[^\]]+)\] '
    r'"(?P<method>[A-Z]+) (?P<target>\S+)(?: HTTP/[0-9.]+)?" '
    r'(?P<status>\d{3}) (?P<size>\d+|-)'
)
CLF_TIME = "%d/%b/%Y:%H:%M:%S %z"


def strip_query(target):
    """Return the path part of a request target."""
    return target.split("?", 1)[0].split("#", 1)[0]


def parse_clf_line(line):
    """Parse one CLF line. Returns an Entry, or None when the line is malformed."""
    m = CLF.match(line.strip())
    if not m:
        return None
    try:
        time = datetime.strptime(m["time"], CLF_TIME)
    except ValueError:
        return None
    size = 0 if m["size"] == "-" else int(m["size"])
    return Entry(
        host=m["host"],
        time=time,
        method=m["method"],
        path=strip_query(m["target"]),
        status=int(m["status"]),
        size=size,
    )


def parse_jsonl_line(line):
    """Parse one JSON lines record. Returns an Entry, or None when it is malformed."""
    try:
        obj = json.loads(line)
    except ValueError:
        return None
    if not isinstance(obj, dict):
        return None
    host, time, method, path = (obj.get(k) for k in ("host", "time", "method", "path"))
    status, size = obj.get("status"), obj.get("bytes")
    if not all(isinstance(v, str) for v in (host, time, method, path)):
        return None
    if not isinstance(status, int) or isinstance(status, bool):
        return None
    if size is None:
        size = 0
    elif not isinstance(size, int) or isinstance(size, bool):
        return None
    try:
        when = datetime.fromisoformat(time)
    except ValueError:
        return None
    if when.tzinfo is None:
        return None
    return Entry(host=host, time=when, method=method, path=strip_query(path),
                 status=status, size=size)


PARSERS = {"clf": parse_clf_line, "jsonl": parse_jsonl_line}
FORMATS = ("auto", "clf", "jsonl")


def detect_format(lines):
    """"jsonl" when the first non-blank line starts with "{", else "clf"."""
    for line in lines:
        if line.strip():
            return "jsonl" if line.lstrip().startswith("{") else "clf"
    return "clf"


class ParseResult:
    """Entries read from one or more logs, and how many lines were skipped."""

    def __init__(self):
        self.entries = []
        self.skipped = 0

    def extend(self, other):
        self.entries.extend(other.entries)
        self.skipped += other.skipped


def parse_lines(lines, fmt="clf"):
    """Parse an iterable of lines in format fmt ("clf" or "jsonl") into a ParseResult."""
    parse_line = PARSERS[fmt]
    result = ParseResult()
    for line in lines:
        if not line.strip():
            continue
        entry = parse_line(line)
        if entry is None:
            result.skipped += 1
        else:
            result.entries.append(entry)
    return result


def read_file(path, fmt="auto"):
    """Parse the log file at path; fmt "auto" detects the format from its first line."""
    with open(path, encoding="utf-8", errors="replace") as f:
        lines = f.readlines()
    if fmt == "auto":
        fmt = detect_format(lines)
    return parse_lines(lines, fmt)


def read_files(paths, fmt="auto"):
    """Parse several log files into one ParseResult, in the order given."""
    result = ParseResult()
    for path in paths:
        result.extend(read_file(path, fmt))
    return result
