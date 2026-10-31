"""Splitting a bill between people."""
from shop.pricing import calc_total as _total


def split_evenly(lines, tax, people):
    """Return a list of shares in cents; earlier people pay the odd cents."""
    if people <= 0:
        raise ValueError("people must be positive")
    total = _total(lines, tax)
    base, rest = divmod(total, people)
    return [base + (1 if i < rest else 0) for i in range(people)]
