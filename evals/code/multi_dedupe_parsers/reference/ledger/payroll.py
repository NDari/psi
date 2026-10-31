"""Importer for payroll runs: blocks of "key: value" lines separated by blank lines."""

from ledger.model import Transaction
from ledger.parsing import parse_amount, parse_date


def load(text):
    out = []
    for block in text.strip().split("\n\n"):
        fields = {}
        for line in block.splitlines():
            key, _, value = line.partition(":")
            fields[key.strip().lower()] = value.strip()
        amount = -parse_amount(fields["net"])
        out.append(Transaction(parse_date(fields["date"]), amount, "salary " + fields["employee"]))
    return out
