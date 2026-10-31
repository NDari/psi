"""Importer for the petty cash sheet: date<TAB>amount<TAB>note, # starts a comment."""

from ledger.model import Transaction
from ledger.parsing import parse_amount, parse_date


def load(text):
    out = []
    for row in text.splitlines():
        if not row.strip() or row.lstrip().startswith("#"):
            continue
        date, amount, note = row.split("\t")
        out.append(Transaction(parse_date(date), parse_amount(amount), note))
    return out
