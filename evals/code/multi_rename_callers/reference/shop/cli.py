"""Command line receipt: python3 -m shop.cli PRICExQTY... [--tax RATE]"""
import argparse

from shop.invoice import render
from shop.pricing import order_total


def parse_line(text):
    price, _, qty = text.partition("x")
    return int(price), int(qty or 1)


def main(argv=None):
    ap = argparse.ArgumentParser(prog="shop")
    ap.add_argument("lines", nargs="+", help="PRICExQTY in cents, e.g. 250x2")
    ap.add_argument("--tax", type=float, default=0.2)
    args = ap.parse_args(argv)
    lines = [parse_line(t) for t in args.lines]
    print(render(lines, args.tax), end="")
    print(f"due {order_total(lines, args.tax)} cents")


if __name__ == "__main__":
    main()
