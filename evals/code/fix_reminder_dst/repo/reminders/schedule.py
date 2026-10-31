"""When daily reminders fire."""
from datetime import datetime, timedelta, timezone
from zoneinfo import ZoneInfo

DAY = timedelta(days=1)


def next_daily(at, tz, after):
    """Next datetime in zone tz, strictly after `after`, when the local clock shows `at`."""
    if after.tzinfo is None:
        raise ValueError("after must be timezone-aware")
    zone = ZoneInfo(tz)
    local = after.astimezone(zone)
    candidate = local.replace(hour=at.hour, minute=at.minute, second=0, microsecond=0)
    if candidate <= local:
        candidate = (candidate.astimezone(timezone.utc) + DAY).astimezone(zone)
    return candidate


def upcoming(at, tz, after, count):
    """The next `count` times the reminder fires after `after`."""
    if count < 0:
        raise ValueError("count must not be negative")
    if count == 0:
        return []
    first = next_daily(at, tz, after)
    start = first.astimezone(timezone.utc)
    return [(start + i * DAY).astimezone(first.tzinfo) for i in range(count)]


def hours_between(start, end):
    """Real elapsed hours from start to end (both aware)."""
    return (end - start).total_seconds() / 3600


def format_countdown(start, end):
    """ "in 25 h 0 min" for the time from start to end."""
    minutes = round(hours_between(start, end) * 60)
    if minutes < 0:
        return "already passed"
    return f"in {minutes // 60} h {minutes % 60} min"
