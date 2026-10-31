"""Parsing log lines into entries."""
import datetime
from typing import NamedTuple

from logsift.config import Config


class Entry(NamedTuple):
    time: datetime.datetime
    level: str
    method: str
    path: str
    ms: int


class ParseError(ValueError):
    pass


def parse_line(line, config=None):
    """Parse one line: '<date> <time> LEVEL METHOD PATH <n>ms'."""
    config = config or Config()
    parts = line.split()
    if len(parts) < 6 or not parts[-1].endswith("ms"):
        raise ParseError(f"cannot parse: {line!r}")
    # the timestamp is everything before the last four fields
    stamp = " ".join(parts[:-4])
    level, method, path, ms = parts[-4:]
    try:
        when = datetime.datetime.strptime(stamp, config.time_format)
        took = int(ms[:-2])
    except ValueError:
        raise ParseError(f"cannot parse: {line!r}") from None
    return Entry(when, level, method, path, took)


def parse_all(lines, config=None):
    """Parse lines, skipping the ones that do not parse. Returns (entries, skipped)."""
    entries, skipped = [], 0
    for line in lines:
        try:
            entries.append(parse_line(line, config))
        except ParseError:
            skipped += 1
    return entries, skipped
