"""Importer for the card statement: columns separated by two or more spaces."""
import re

from ledger.model import Transaction
from ledger.parsing import parse_amount, parse_date


def load(text):
    out = []
    for row in text.splitlines():
        if not row.strip() or row.startswith("DATE"):
            continue
        date, desc, amount = re.split(r"\s{2,}", row.strip())
        # card statements show purchases as positive numbers
        out.append(Transaction(parse_date(date), -parse_amount(amount), desc))
    return out
