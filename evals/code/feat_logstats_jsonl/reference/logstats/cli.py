"""Command line: python3 -m logstats [options] FILE..."""
import argparse
import sys

from . import __version__, filters, parse, report, stats


def build_parser():
    p = argparse.ArgumentParser(prog="logstats", description="Summarize web server access logs.")
    p.add_argument("files", nargs="+", metavar="FILE", help="access log files")
    p.add_argument("--top", type=int, default=5, metavar="N",
                   help="how many paths and hosts to list (default 5)")
    p.add_argument("--since", metavar="WHEN",
                   help="only requests at or after this ISO 8601 date or time (UTC if no offset)")
    p.add_argument("--status", metavar="CLASS",
                   help='only requests with this status ("404") or class ("5xx")')
    p.add_argument("--format", choices=parse.FORMATS, default="auto",
                   help="log format: clf, jsonl, or auto to detect it per file (default)")
    p.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        result = parse.read_files(args.files, args.format)
    except OSError as e:
        print(f"logstats: {e.filename}: {e.strerror}", file=sys.stderr)
        return 1
    entries = result.entries
    if args.since:
        try:
            start = filters.parse_since(args.since)
        except ValueError:
            print(f"logstats: bad --since value: {args.since!r}", file=sys.stderr)
            return 2
        entries = filters.since(entries, start)
    if args.status:
        entries = filters.status_class(entries, args.status)
    summary = stats.summarize(entries, skipped=result.skipped)
    sys.stdout.write(report.render(summary, top=args.top))
    return 0
