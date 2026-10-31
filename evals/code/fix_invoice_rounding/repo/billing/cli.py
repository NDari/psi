"""python3 -m billing ORDER.json"""
import json
import sys

from .invoice import from_order
from .render import render


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) != 1:
        print("usage: python3 -m billing ORDER.json", file=sys.stderr)
        return 2
    try:
        with open(argv[0], encoding="utf-8") as f:
            order = json.load(f)
        print(render(from_order(order)))
    except (OSError, json.JSONDecodeError, ValueError) as e:
        print(f"billing: {e}", file=sys.stderr)
        return 1
    return 0
