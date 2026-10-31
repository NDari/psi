"""python3 -m reminders list --at HH:MM --tz ZONE --from YYYY-MM-DDTHH:MM [--count N]"""
import argparse
import sys
from datetime import datetime, time
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from .schedule import format_countdown, upcoming


def parse_args(argv):
    parser = argparse.ArgumentParser(prog="reminders")
    sub = parser.add_subparsers(dest="command", required=True)
    ls = sub.add_parser("list", help="show the next reminder times")
    ls.add_argument("--at", required=True, type=time.fromisoformat, help="local time, HH:MM")
    ls.add_argument("--tz", required=True, help="IANA zone, e.g. Europe/Berlin")
    ls.add_argument("--from", dest="start", type=datetime.fromisoformat,
                    help="local start time in --tz (default: now)")
    ls.add_argument("--count", type=int, default=5)
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    try:
        zone = ZoneInfo(args.tz)
    except (ZoneInfoNotFoundError, ValueError):
        print(f"reminders: unknown time zone {args.tz!r}", file=sys.stderr)
        return 2
    start = args.start.replace(tzinfo=zone) if args.start else datetime.now(zone)
    for when in upcoming(args.at, args.tz, start, args.count):
        print(f"{when:%Y-%m-%d %H:%M %Z} ({format_countdown(start, when)})")
    return 0
