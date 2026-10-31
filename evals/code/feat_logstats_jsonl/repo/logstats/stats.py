"""Turning entries into a summary."""
from collections import Counter
from dataclasses import dataclass, field
from datetime import timezone

STATUS_CLASSES = ("1xx", "2xx", "3xx", "4xx", "5xx")


@dataclass
class Summary:
    requests: int = 0
    skipped: int = 0
    bytes: int = 0
    by_class: Counter = field(default_factory=Counter)
    by_path: Counter = field(default_factory=Counter)
    by_host: Counter = field(default_factory=Counter)
    by_hour: Counter = field(default_factory=Counter)  # UTC hour start -> requests

    def top(self, counter, n):
        """The n largest items of counter, ties broken by key."""
        return sorted(counter.items(), key=lambda kv: (-kv[1], kv[0]))[:n]

    def busiest_hour(self):
        """(hour, requests) for the hour with most requests, earliest on ties; None if empty."""
        if not self.by_hour:
            return None
        return min(self.by_hour.items(), key=lambda kv: (-kv[1], kv[0]))


def hour_of(entry):
    t = entry.time.astimezone(timezone.utc)
    return t.replace(minute=0, second=0, microsecond=0)


def summarize(entries, skipped=0):
    s = Summary(skipped=skipped)
    for e in entries:
        s.requests += 1
        s.bytes += e.size
        s.by_class[e.status_class] += 1
        s.by_path[e.path] += 1
        s.by_host[e.host] += 1
        s.by_hour[hour_of(e)] += 1
    return s
