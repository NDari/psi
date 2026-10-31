"""Importer for Bank A CSV exports: a header line, then date,description,amount."""
import datetime

from ledger.model import Transaction


def _parse_amount(text):
    """Parse "1,234.50", "-3", "$12.05" or "(7.25)" into integer cents."""
    s = text.strip().replace(",", "")
    negative = False
    if s.startswith("(") and s.endswith(")"):
        negative = True
        s = s[1:-1].strip()
    if s.startswith("-"):
        negative = not negative
        s = s[1:].strip()
    if s.startswith("$"):
        s = s[1:]
    whole, dot, frac = s.partition(".")
    if not whole.isdigit() or (dot and not (frac.isdigit() and len(frac) <= 2)):
        raise ValueError(f"bad amount: {text!r}")
    cents = int(whole) * 100 + int(frac.ljust(2, "0") if frac else 0)
    return -cents if negative else cents


def _parse_date(text):
    """Parse "2024-03-09" or "09/03/2024" (day first) into a date."""
    s = text.strip()
    try:
        if "/" in s:
            day, month, year = s.split("/")
        else:
            year, month, day = s.split("-")
        if len(year) != 4:
            raise ValueError
        return datetime.date(int(year), int(month), int(day))
    except ValueError:
        raise ValueError(f"bad date: {text!r}") from None


def load(text):
    rows = text.strip().splitlines()[1:]
    out = []
    for row in rows:
        date, desc, amount = row.rsplit(",", 2) if row.count(",") == 2 else _split_quoted(row)
        out.append(Transaction(_parse_date(date), _parse_amount(amount), desc.strip()))
    return out


def _split_quoted(row):
    # Bank A quotes the amount when it has a thousands separator: 2024-01-02,Rent,"1,200.00"
    head, _, amount = row.partition(",\"")
    date, _, desc = head.partition(",")
    return date, desc, amount.rstrip("\"")
