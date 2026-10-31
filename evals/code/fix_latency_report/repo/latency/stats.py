"""Summary statistics over Records."""
import math
from dataclasses import dataclass


@dataclass
class Summary:
    count: int
    mean: float
    p50: int | None
    p95: int | None
    max: int
    error_rate: float


def percentile(sorted_values, pct):
    """Nearest-rank percentile of an already sorted list (None if empty)."""
    if not sorted_values:
        return None
    rank = max(1, math.ceil(pct / 100 * len(sorted_values)))
    return sorted_values[rank - 1]


def summarize(records):
    """Summarize an iterable of Records. Returns None when there are none."""
    count = 0
    total = 0
    errors = 0
    for r in records:
        count += 1
        total += r.ms
        if r.status >= 500:
            errors += 1
    if count == 0:
        return None
    latencies = sorted(r.ms for r in records)
    return Summary(
        count=count,
        mean=total / count,
        p50=percentile(latencies, 50),
        p95=percentile(latencies, 95),
        max=max(latencies, default=0),
        error_rate=errors / count,
    )


def only_endpoint(records, path):
    """The records for one path."""
    return (r for r in records if r.path == path)


def by_endpoint(records):
    """{path: Summary} for every path in records."""
    records = list(records)
    paths = sorted({r.path for r in records})
    return {p: summarize(only_endpoint(records, p)) for p in paths}
