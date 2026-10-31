"""Command line front end for the notes tools.

Usage:
    python3 app.py slug TEXT
    python3 app.py case snake|kebab|camel|title TEXT
    python3 app.py stats FILE
    python3 app.py wrap WIDTH FILE
    python3 app.py report DIRECTORY
"""

import sys

from textkit import (
    camel_case,
    fill,
    kebab_case,
    slugify,
    snake_case,
    summary,
    title_case,
)
from report import build_report, load_notes

CASES = {
    "snake": snake_case,
    "kebab": kebab_case,
    "camel": camel_case,
    "title": title_case,
}


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def main(argv):
    if len(argv) < 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    cmd, args = argv[1], argv[2:]
    if cmd == "slug" and len(args) == 1:
        print(slugify(args[0]))
    elif cmd == "case" and len(args) == 2 and args[0] in CASES:
        print(CASES[args[0]](args[1]))
    elif cmd == "stats" and len(args) == 1:
        info = summary(read(args[0]))
        for key in ("words", "characters", "sentences", "reading_minutes", "readability"):
            print("%-16s %s" % (key, info[key]))
        print("%-16s %s" % ("top", ", ".join("%s=%d" % wc for wc in info["top"])))
    elif cmd == "wrap" and len(args) == 2:
        print(fill(read(args[1]), int(args[0])))
    elif cmd == "report" and len(args) == 1:
        print(build_report(load_notes(args[0])), end="")
    else:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
