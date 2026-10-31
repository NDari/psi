#!/usr/bin/env python3
"""Count words in text files.

    python3 tally.py [FILE...]

Reads standard input when no file is given. Prints one line per distinct
word, "COUNT WORD", most frequent first; ties are ordered alphabetically.
"""
import argparse
import re
import sys
from collections import Counter

WORD = re.compile(r"[A-Za-z']+")


def words(text, ignore_case=False):
    for w in WORD.findall(text):
        w = w.strip("'")
        if not w:
            continue
        yield w.lower() if ignore_case else w


def count(texts, ignore_case=False):
    counts = Counter()
    for text in texts:
        counts.update(words(text, ignore_case))
    return counts


def ranked(counts):
    return sorted(counts.items(), key=lambda item: (-item[1], item[0]))


def read_inputs(paths):
    if not paths:
        yield sys.stdin.read()
        return
    for path in paths:
        with open(path, encoding="utf-8") as f:
            yield f.read()


def positive_int(text):
    try:
        n = int(text)
    except ValueError:
        raise argparse.ArgumentTypeError(f"not an integer: {text!r}")
    if n < 1:
        raise argparse.ArgumentTypeError(f"must be at least 1: {n}")
    return n


def non_negative_int(text):
    try:
        n = int(text)
    except ValueError:
        raise argparse.ArgumentTypeError(f"not an integer: {text!r}")
    if n < 0:
        raise argparse.ArgumentTypeError(f"must not be negative: {n}")
    return n


def main(argv=None):
    parser = argparse.ArgumentParser(prog="tally.py", description="Count words in text files.")
    parser.add_argument("files", nargs="*", metavar="FILE")
    parser.add_argument("-i", "--ignore-case", action="store_true",
                        help="count words case-insensitively (printed in lower case)")
    parser.add_argument("--top", type=positive_int, metavar="N",
                        help="print only the N most frequent words")
    parser.add_argument("--min-length", type=non_negative_int, default=0, metavar="L",
                        help="ignore words shorter than L characters")
    args = parser.parse_args(argv)
    counts = count(read_inputs(args.files), args.ignore_case)
    lines = [(w, n) for w, n in ranked(counts) if len(w) >= args.min_length]
    if args.top is not None:
        lines = lines[:args.top]
    for word, n in lines:
        print(f"{n} {word}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
