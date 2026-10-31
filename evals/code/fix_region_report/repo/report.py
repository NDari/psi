#!/usr/bin/env python3
"""Sales totals per region from a CSV export."""
import argparse
import csv
import sys
from decimal import Decimal, InvalidOperation


def load(path):
    """Return {region: Decimal total} from a CSV with date,region,amount."""
    totals = {}
    with open(path, newline="") as f:
        for lineno, row in enumerate(csv.DictReader(f), start=2):
            region = (row.get("region") or "").strip()
            try:
                amount = Decimal(row["amount"].strip())
            except (InvalidOperation, AttributeError, KeyError):
                print(f"{path}:{lineno}: bad amount, skipped", file=sys.stderr)
                continue
            if not region:
                print(f"{path}:{lineno}: no region, skipped", file=sys.stderr)
                continue
            totals[region] = totals.get(region, Decimal(0)) + amount
    return totals


def ranked(totals):
    """Regions with their totals, biggest total first."""
    return sorted(totals.items(), key=lambda item: (item[1], item[0]))


def render(totals, top=None):
    rows = ranked(totals)
    if top is not None:
        rows = rows[:top]
    lines = [f"{region:<12}{total:>12.2f}" for region, total in rows]
    lines.append(f"{'TOTAL':<12}{sum(totals.values(), Decimal(0)):>12.2f}")
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("csv")
    parser.add_argument("--top", type=int, default=None, help="show only the N biggest regions")
    args = parser.parse_args(argv)
    try:
        totals = load(args.csv)
    except OSError as e:
        print(f"report: {e}", file=sys.stderr)
        return 1
    print(render(totals, args.top))
    return 0


if __name__ == "__main__":
    sys.exit(main())
