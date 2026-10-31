"""Command line: python3 -m latency report ACCESS.log [--endpoint PATH] [--by-endpoint]"""
import argparse
import sys

from .loader import Skipped, read_log
from .stats import by_endpoint, only_endpoint, summarize


def fmt_ms(value):
    return "n/a" if value is None else f"{value}ms"


def format_summary(label, s):
    if s is None:
        return f"{label}: no requests"
    return (f"{label}: {s.count} requests, mean {s.mean:.1f}ms, p50 {fmt_ms(s.p50)}, "
            f"p95 {fmt_ms(s.p95)}, max {fmt_ms(s.max)}, errors {s.error_rate:.1%}")


def main(argv=None):
    parser = argparse.ArgumentParser(prog="latency")
    sub = parser.add_subparsers(dest="command", required=True)
    rep = sub.add_parser("report", help="summarize an access log")
    rep.add_argument("log")
    rep.add_argument("--endpoint", help="only requests to this path")
    rep.add_argument("--by-endpoint", action="store_true", help="one line per path")
    args = parser.parse_args(argv)

    skipped = Skipped()
    try:
        records = read_log(args.log, skipped)
        if args.by_endpoint:
            for path, summary in by_endpoint(records).items():
                print(format_summary(path, summary))
        else:
            if args.endpoint:
                records = only_endpoint(records, args.endpoint)
            print(format_summary(args.endpoint or "all", summarize(records)))
    except OSError as e:
        print(f"latency: {e}", file=sys.stderr)
        return 1
    if skipped.count:
        print(f"skipped {skipped.count} malformed lines", file=sys.stderr)
    return 0
