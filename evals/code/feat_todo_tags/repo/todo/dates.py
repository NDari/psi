"""Dates as the user types them.

"today" is normally the system date; the TODO_TODAY environment variable
(YYYY-MM-DD) overrides it, which keeps tests and demos reproducible.
"""
import os
from datetime import date, timedelta

from .model import TodoError

WEEKDAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]


def today():
    text = os.environ.get("TODO_TODAY")
    if text:
        return date.fromisoformat(text)
    return date.today()


def parse_due(text):
    """Turn "today", "tomorrow", a weekday name ("fri": the next one, never
    today) or YYYY-MM-DD into an ISO date string."""
    word = text.strip().lower()
    base = today()
    if word == "today":
        return base.isoformat()
    if word == "tomorrow":
        return (base + timedelta(days=1)).isoformat()
    if word[:3] in WEEKDAYS and len(word) >= 3:
        ahead = (WEEKDAYS.index(word[:3]) - base.weekday() - 1) % 7 + 1
        return (base + timedelta(days=ahead)).isoformat()
    try:
        return date.fromisoformat(word).isoformat()
    except ValueError:
        raise TodoError(f"bad date: {text}") from None


def is_overdue(due):
    return due is not None and date.fromisoformat(due) < today()
