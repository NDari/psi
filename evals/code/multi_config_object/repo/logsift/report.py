"""The text report."""
from logsift.analyze import error_count, slow_requests, stats_by_path


def render(entries, skipped=0):
    """Return the report text for parsed entries."""
    if not entries:
        return "no entries\n"
    first = min(e.time for e in entries)
    last = max(e.time for e in entries)
    lines = [
        f"from {first.strftime('%Y-%m-%d %H:%M:%S')} to {last.strftime('%Y-%m-%d %H:%M:%S')}",
        f"requests {len(entries)}, errors {error_count(entries)}, skipped lines {skipped}",
    ]
    slow = slow_requests(entries)
    lines.append(f"slow requests (>= 500 ms): {len(slow)}")
    stats = stats_by_path(entries)
    ranked = sorted(stats.items(), key=lambda kv: (-kv[1][1], kv[0]))[:5]
    lines.append("slowest paths:")
    for path, (count, avg, worst) in ranked:
        lines.append(f"  {path} avg {avg} ms, max {worst} ms, {count} calls")
    return "\n".join(lines) + "\n"
