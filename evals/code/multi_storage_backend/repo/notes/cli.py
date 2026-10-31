"""Command line: python3 -m notes --store URL COMMAND ..."""
import argparse
import sys
import time

from notes.model import Note
from notes.store import open_store


def main(argv=None):
    ap = argparse.ArgumentParser(prog="notes")
    ap.add_argument("--store", default="json:notes.json", help="store URL (default json:notes.json)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("add")
    a.add_argument("key")
    a.add_argument("title")
    a.add_argument("body", nargs="?", default="")
    a.add_argument("--tag", action="append", default=[])
    sub.add_parser("list")
    s = sub.add_parser("show")
    s.add_argument("key")
    r = sub.add_parser("rm")
    r.add_argument("key")
    f = sub.add_parser("find")
    f.add_argument("text")
    args = ap.parse_args(argv)

    try:
        store = open_store(args.store)
        if args.cmd == "add":
            store.put(args.key, Note(args.title, args.body, tuple(args.tag), int(time.time())))
        elif args.cmd == "list":
            for k in store.keys():
                print(f"{k}\t{store.get(k).title}")
        elif args.cmd == "show":
            n = store.get(args.key)
            print(n.title)
            if n.tags:
                print("tags: " + ", ".join(n.tags))
            if n.body:
                print()
                print(n.body)
        elif args.cmd == "rm":
            store.delete(args.key)
        elif args.cmd == "find":
            for k in store.search(args.text):
                print(k)
    except KeyError as e:
        print(f"notes: no note {e}", file=sys.stderr)
        return 1
    except ValueError as e:
        print(f"notes: {e}", file=sys.stderr)
        return 2
    return 0
