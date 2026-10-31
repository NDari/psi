"""Money amounts as integer cents."""
import re

_AMOUNT = re.compile(r"\d+(\.\d{1,2})?")


def parse_amount(text):
    """Parse a decimal string such as "12.5" into cents (1250)."""
    text = str(text).strip()
    if not _AMOUNT.fullmatch(text):
        raise ValueError(f"bad amount: {text!r}")
    return int(float(text) * 100)


def format_money(cents):
    """Format cents as "1,234.56" (with a leading "-" when negative)."""
    sign = "-" if cents < 0 else ""
    whole, frac = divmod(abs(cents), 100)
    return f"{sign}{whole:,}.{frac:02d}"
