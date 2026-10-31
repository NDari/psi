"""Parsing and describing dates relative to a given 'today'.

Nothing here reads the clock: callers pass today in, which keeps the
command line testable with --today.
"""

from __future__ import annotations

import re
from datetime import date, timedelta

WEEKDAYS = ("mon", "tue", "wed", "thu", "fri", "sat", "sun")

_OFFSET_RE = re.compile(r"^([+-])(\d{1,4})([dw])$")


def parse_date(text: str, today: date) -> date:
    """Parse a date written as:

    - YYYY-MM-DD
    - today, tomorrow, yesterday
    - +Nd / -Nd (days) or +Nw / -Nw (weeks) from today
    - a weekday name (mon .. sun, or the full name): the next such day
      strictly after today
    - eow: the coming Sunday (today when today is a Sunday)

    Raises ValueError for anything else.
    """
    s = text.strip().lower()
    if not s:
        raise ValueError("empty date")
    if s == "today":
        return today
    if s == "tomorrow":
        return today + timedelta(days=1)
    if s == "yesterday":
        return today - timedelta(days=1)
    if s == "eow":
        return today + timedelta(days=(6 - today.weekday()))
    m = _OFFSET_RE.match(s)
    if m:
        sign, n, unit = m.groups()
        days = int(n) * (7 if unit == "w" else 1)
        return today + timedelta(days=days if sign == "+" else -days)
    day = _weekday_index(s)
    if day is not None:
        ahead = (day - today.weekday()) % 7 or 7
        return today + timedelta(days=ahead)
    try:
        return date.fromisoformat(s)
    except ValueError:
        raise ValueError(f"not a date: {text!r}") from None


def _weekday_index(s: str) -> int | None:
    for i, name in enumerate(WEEKDAYS):
        full = date(2024, 1, 1 + i).strftime("%A").lower()  # 2024-01-01 is a Monday
        if s == name or s == full:
            return i
    return None


def format_date(d: date | None) -> str:
    return d.isoformat() if d else "-"


def days_until(d: date, today: date) -> int:
    return (d - today).days


def humanize(d: date | None, today: date) -> str:
    """A short relative description: today, tomorrow, yesterday,
    in N days, N days ago."""
    if d is None:
        return "no date"
    n = days_until(d, today)
    if n == 0:
        return "today"
    if n == 1:
        return "tomorrow"
    if n == -1:
        return "yesterday"
    if n > 0:
        return f"in {n} days"
    return f"{-n} days ago"


def week_range(today: date) -> tuple[date, date]:
    """The seven days after today, as (first, last) inclusive."""
    return today + timedelta(days=1), today + timedelta(days=7)
