"""Reading log files."""


def read_lines(path):
    """Return the non-blank lines of the file at path, without line endings.

    Very long lines (stack traces glued onto one line) are cut to 400
    characters so one bad line cannot blow up the report.
    """
    out = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for raw in f:
            line = raw.rstrip("\n")
            if not line.strip():
                continue
            out.append(line[:400])
    return out
