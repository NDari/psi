"""VAT."""
import re

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
    # Use floor(x + 0.5) for round-half-up behavior
    return int(net_cents * float(rate) / 100 + 0.5)
