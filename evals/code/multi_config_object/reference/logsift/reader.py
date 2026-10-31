"""Reading log files."""
from logsift.config import Config


def read_lines(path, config=None):
    """Return the non-blank lines of the file at path, without line endings.

    Very long lines (stack traces glued onto one line) are cut to
    config.max_line_length characters so one bad line cannot blow up the report.
    """
    config = config or Config()
    out = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for raw in f:
            line = raw.rstrip("\n")
            if not line.strip():
                continue
            out.append(line[:config.max_line_length])
    return out
