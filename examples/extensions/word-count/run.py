#!/usr/bin/env python3
"""A script tool (D-22): arguments as JSON on stdin, the result on stdout.
A non-zero exit marks the call as failed."""
import json
import sys

args = json.load(sys.stdin)
try:
    with open(args["path"], encoding="utf-8") as f:
        print(len(f.read().split()))
except OSError as e:
    print(f"cannot read {args['path']}: {e.strerror}")
    sys.exit(1)
