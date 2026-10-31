"""The text report."""
from logsift.analyze import error_count, slow_requests, stats_by_path
from logsift.config import Config


def render(entries, skipped=0, config=None):
    """Return the report text for parsed entries."""
    config = config or Config()
    if not entries:
        return "no entries\n"
    first = min(e.time for e in entries)
    last = max(e.time for e in entries)
    lines = [
        f"from {first.strftime(config.time_format)} to {last.strftime(config.time_format)}",
        f"requests {len(entries)}, errors {error_count(entries)}, skipped lines {skipped}",
    ]
    slow = slow_requests(entries, config)
    lines.append(f"slow requests (>= {config.slow_ms} ms): {len(slow)}")
    stats = stats_by_path(entries)
    ranked = sorted(stats.items(), key=lambda kv: (-kv[1][1], kv[0]))[:config.top_n]
    lines.append("slowest paths:")
    for path, (count, avg, worst) in ranked:
        lines.append(f"  {path} avg {avg} ms, max {worst} ms, {count} calls")
    return "\n".join(lines) + "\n"
