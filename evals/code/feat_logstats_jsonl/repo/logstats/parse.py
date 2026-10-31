"""Reading access logs.

The supported format is the Common Log Format (CLF), as written by Apache
and nginx:

    10.0.0.1 - frank [10/Oct/2026:13:55:36 +0200] "GET /a.gif?x=1 HTTP/1.1" 200 2326

Lines that cannot be parsed are counted as skipped, not fatal: real logs
contain junk. Blank lines are ignored and not counted.
"""
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


class ParseResult:
    """Entries read from one or more logs, and how many lines were skipped."""

    def __init__(self):
        self.entries = []
        self.skipped = 0

    def extend(self, other):
        self.entries.extend(other.entries)
        self.skipped += other.skipped


def parse_lines(lines):
    """Parse an iterable of CLF lines into a ParseResult."""
    result = ParseResult()
    for line in lines:
        if not line.strip():
            continue
        entry = parse_clf_line(line)
        if entry is None:
            result.skipped += 1
        else:
            result.entries.append(entry)
    return result


def read_file(path):
    """Parse the log file at path."""
    with open(path, encoding="utf-8", errors="replace") as f:
        return parse_lines(f)


def read_files(paths):
    """Parse several log files into one ParseResult, in the order given."""
    result = ParseResult()
    for path in paths:
        result.extend(read_file(path))
    return result
