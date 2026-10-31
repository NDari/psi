"""Command line entry point: python3 -m logsift FILE"""
import argparse
import sys

from logsift.parser import parse_all
from logsift.reader import read_lines
from logsift.report import render


def main(argv=None):
    ap = argparse.ArgumentParser(prog="logsift", description="Summarise an access log.")
    ap.add_argument("file")
    args = ap.parse_args(argv)
    try:
        lines = read_lines(args.file)
    except OSError as e:
        print(f"logsift: {e}", file=sys.stderr)
        return 1
    entries, skipped = parse_all(lines)
    sys.stdout.write(render(entries, skipped))
    return 0
