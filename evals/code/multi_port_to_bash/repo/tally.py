#!/usr/bin/env python3
"""Count log lines per level and the noisiest components.

usage: tally [-l LEVEL] FILE...

A log line looks like "2024-05-01 10:00:03 WARN db: slow query". Lines that
do not have that shape are ignored. With -l, only lines of that level count.
"""
import re
import sys
from collections import Counter

LINE = re.compile(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} ([A-Z]+) ([a-z0-9_-]+): ")
USAGE = "usage: tally [-l LEVEL] FILE..."


def main(argv):
    args = argv[1:]
    level = None
    if args[:1] == ["-l"]:
        if len(args) < 2:
            print(USAGE, file=sys.stderr)
            return 2
        level, args = args[1], args[2:]
    if not args:
        print(USAGE, file=sys.stderr)
        return 2

    levels, components = Counter(), Counter()
    for path in args:
        try:
            with open(path, encoding="utf-8") as f:
                lines = f.read().splitlines()
        except OSError:
            print(f"tally: cannot read {path}", file=sys.stderr)
            return 1
        for line in lines:
            m = LINE.match(line)
            if not m or (level and m.group(1) != level):
                continue
            levels[m.group(1)] += 1
            components[m.group(2)] += 1

    print(f"total {sum(levels.values())}")
    for name in sorted(levels):
        print(f"{name} {levels[name]}")
    print("top components:")
    for name, n in sorted(components.items(), key=lambda kv: (-kv[1], kv[0]))[:3]:
        print(f"  {name} {n}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
