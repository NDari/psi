"""Turn rows of a CSV file into small reports.

Rows are dicts mapping column name to value, as csv.DictReader gives them.
"""
import csv
import io
import sys


def load_rows(path):
    """Read a CSV file with a header line into a list of dicts."""
    with open(path, newline="", encoding="utf-8") as f:
        return list(csv.DictReader(f))


def _cell(row, column):
    value = row.get(column)
    return "" if value is None else str(value)


def to_csv(rows, columns):
    """Return the rows as CSV text with only the given columns."""
    out = io.StringIO()
    writer = csv.writer(out, lineterminator="\n")
    writer.writerow(columns)
    for row in rows:
        writer.writerow([_cell(row, c) for c in columns])
    return out.getvalue()


def to_text(rows, columns):
    """Return the rows as a plain text table, columns padded to equal width."""
    table = [list(columns)] + [[_cell(row, c) for c in columns] for row in rows]
    widths = [max(len(line[i]) for line in table) for i in range(len(columns))]
    lines = []
    for line in table:
        lines.append("  ".join(v.ljust(w) for v, w in zip(line, widths)).rstrip())
    return "\n".join(lines) + "\n"


def main(argv):
    if len(argv) < 2:
        print("usage: report.py FILE.csv [COLUMN...]", file=sys.stderr)
        return 2
    rows = load_rows(argv[1])
    columns = argv[2:] or (list(rows[0].keys()) if rows else [])
    sys.stdout.write(to_text(rows, columns))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
