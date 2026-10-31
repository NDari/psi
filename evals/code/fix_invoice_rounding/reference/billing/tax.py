"""VAT."""
import re
from decimal import ROUND_HALF_UP, Decimal

_RATE = re.compile(r"\d{1,2}(\.\d{1,2})?")


def check_rate(rate):
    """Validate a VAT rate given as a percent string, return it normalized."""
    rate = str(rate).strip()
    if not _RATE.fullmatch(rate):
        raise ValueError(f"bad VAT rate: {rate!r}")
    return rate


def line_vat(net_cents, rate):
    """VAT in cents on a net amount in cents at `rate` percent, halves rounded up."""
    rate = check_rate(rate)
    vat = Decimal(net_cents) * Decimal(rate) / 100
    return int(vat.quantize(Decimal(1), rounding=ROUND_HALF_UP))
