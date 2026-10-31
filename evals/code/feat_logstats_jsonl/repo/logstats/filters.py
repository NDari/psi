"""Selecting which entries a report covers."""
from datetime import datetime, timezone


def parse_since(text):
    """Parse a --since value: an ISO 8601 date or date-time.

    A value without a UTC offset is taken as UTC.
    """
    t = datetime.fromisoformat(text)
    if t.tzinfo is None:
        t = t.replace(tzinfo=timezone.utc)
    return t


def since(entries, start):
    return [e for e in entries if e.time >= start]


def status_class(entries, cls):
    """Keep entries whose status class is cls ("4xx") or whose status is cls ("404")."""
    if cls.endswith("xx"):
        return [e for e in entries if e.status_class == cls]
    return [e for e in entries if str(e.status) == cls]
