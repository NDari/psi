"""Importer for Bank A CSV exports: a header line, then date,description,amount."""

from ledger.model import Transaction
from ledger.parsing import parse_amount, parse_date


def load(text):
    rows = text.strip().splitlines()[1:]
    out = []
    for row in rows:
        date, desc, amount = row.rsplit(",", 2) if row.count(",") == 2 else _split_quoted(row)
        out.append(Transaction(parse_date(date), parse_amount(amount), desc.strip()))
    return out


def _split_quoted(row):
    # Bank A quotes the amount when it has a thousands separator: 2024-01-02,Rent,"1,200.00"
    head, _, amount = row.partition(",\"")
    date, _, desc = head.partition(",")
    return date, desc, amount.rstrip("\"")
