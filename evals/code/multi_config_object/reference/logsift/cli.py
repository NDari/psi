"""Command line entry point: python3 -m logsift FILE"""
import argparse
import sys

from logsift.config import Config, load_config
from logsift.parser import parse_all
from logsift.reader import read_lines
from logsift.report import render


def main(argv=None):
    ap = argparse.ArgumentParser(prog="logsift", description="Summarise an access log.")
    ap.add_argument("file")
    ap.add_argument("--config", metavar="PATH", help="JSON settings file")
    args = ap.parse_args(argv)
    try:
        config = load_config(args.config) if args.config else Config()
        lines = read_lines(args.file, config)
    except (OSError, ValueError) as e:
        print(f"logsift: {e}", file=sys.stderr)
        return 1
    entries, skipped = parse_all(lines, config)
    sys.stdout.write(render(entries, skipped, config))
    return 0
