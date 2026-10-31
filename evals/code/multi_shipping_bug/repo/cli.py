"""Command line quotes.

    python3 cli.py DE FR 2 30x20x10 --value 600 --date 2026-03-02
"""

import argparse
import sys
from datetime import date

from shipping import SERVICES, Parcel, QuoteError, format_quote, quote


def parse_dims(text: str) -> tuple[float, float, float]:
    parts = text.lower().split("x")
    if len(parts) != 3:
        raise argparse.ArgumentTypeError("dimensions look like 30x20x10 (cm)")
    try:
        return tuple(float(p) for p in parts)
    except ValueError:
        raise argparse.ArgumentTypeError("dimensions must be numbers") from None


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description="Quote a parcel.")
    ap.add_argument("origin", help="origin country, e.g. DE")
    ap.add_argument("destination", help="destination country, e.g. FR")
    ap.add_argument("weight", type=float, help="actual weight in kg")
    ap.add_argument("dims", type=parse_dims, help="LxWxH in cm, e.g. 30x20x10")
    ap.add_argument("--service", default="standard", choices=SERVICES)
    ap.add_argument("--value", type=float, default=0.0, help="declared value in EUR")
    ap.add_argument("--postcode", default="", help="destination postcode")
    ap.add_argument("--date", type=date.fromisoformat, default=None, help="ship date, YYYY-MM-DD")
    ap.add_argument("--saturday", action="store_true", help="Saturday delivery")
    ap.add_argument("--account", default="", help="account tier")
    ap.add_argument("--promo", default="", help="promo code")
    args = ap.parse_args(argv)

    length, width, height = args.dims
    parcel = Parcel(args.weight, length, width, height, declared_value=args.value)
    try:
        q = quote(
            args.origin,
            args.destination,
            parcel,
            service=args.service,
            ship_date=args.date,
            postcode=args.postcode,
            saturday=args.saturday,
            account=args.account,
            promo=args.promo,
        )
    except QuoteError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    print(format_quote(q))
    return 0


if __name__ == "__main__":
    sys.exit(main())
