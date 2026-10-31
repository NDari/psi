"""Statistics over parsed entries."""
from collections import defaultdict

from logsift.config import Config


def slow_requests(entries, config=None):
    """Entries that took config.slow_ms or more, slowest first (ties keep log order)."""
    config = config or Config()
    slow = [e for e in entries if e.ms >= config.slow_ms]
    return sorted(slow, key=lambda e: -e.ms)


def stats_by_path(entries):
    """Map path -> (count, average ms rounded down, max ms)."""
    groups = defaultdict(list)
    for e in entries:
        groups[e.path].append(e.ms)
    return {p: (len(v), sum(v) // len(v), max(v)) for p, v in groups.items()}


def error_count(entries):
    return sum(1 for e in entries if e.level == "ERROR")
