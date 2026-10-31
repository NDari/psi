"""Importer for Bank B: amount;date;payee with no header."""

from ledger.model import Transaction
from ledger.parsing import parse_amount, parse_date


def load(text):
    out = []
    for row in text.splitlines():
        if not row.strip():
            continue
        amount, date, payee = row.split(";")
        out.append(Transaction(parse_date(date), parse_amount(amount), payee.strip()))
    return out
