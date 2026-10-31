"""Plain text rendering of a Summary."""
from .stats import STATUS_CLASSES


def render(summary, top=5):
    lines = [
        f"requests: {summary.requests}",
        f"skipped: {summary.skipped}",
        f"bytes: {summary.bytes}",
    ]
    classes = " ".join(f"{c}={summary.by_class[c]}" for c in STATUS_CLASSES if summary.by_class[c])
    lines.append(f"status: {classes or '-'}")
    for title, counter in (("top paths", summary.by_path), ("top hosts", summary.by_host)):
        lines.append(f"{title}:")
        for key, n in summary.top(counter, top):
            lines.append(f"  {n:>5} {key}")
    busiest = summary.busiest_hour()
    if busiest is None:
        lines.append("busiest hour: -")
    else:
        hour, n = busiest
        lines.append(f"busiest hour: {hour:%Y-%m-%d %H}:00 UTC ({n} requests)")
    return "\n".join(lines) + "\n"
