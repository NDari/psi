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


def main(argv=None):
    parser = argparse.ArgumentParser(prog="tally.py", description="Count words in text files.")
    parser.add_argument("files", nargs="*", metavar="FILE")
    parser.add_argument("-i", "--ignore-case", action="store_true",
                        help="count words case-insensitively (printed in lower case)")
    args = parser.parse_args(argv)
    counts = count(read_inputs(args.files), args.ignore_case)
    for word, n in ranked(counts):
        print(f"{n} {word}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
