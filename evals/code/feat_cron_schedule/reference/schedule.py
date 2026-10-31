"""Schedules for the job runner.

A schedule is any object with a `next_after(dt)` method that returns the
first run time strictly after the naive datetime `dt`. The runner only
uses that method, so new kinds of schedule fit in without other changes.

Schedules are written in jobs.conf as text and read by parse_schedule():

    every 15m          -> Every(minutes=15)
    every 2h           -> Every(hours=2)
    daily 06:30        -> Daily(6, 30)
    cron 0 9 * * 1-5   -> Cron("0 9 * * 1-5")
"""
import re
from datetime import datetime, timedelta


class Every:
    """Run at fixed intervals, aligned to midnight.

    Every(minutes=15) runs at 00:00, 00:15, 00:30, ... each day.
    The interval must divide a day evenly.
    """

    def __init__(self, minutes=0, hours=0):
        self.interval = timedelta(hours=hours, minutes=minutes)
        if self.interval <= timedelta(0):
            raise ValueError("interval must be positive")
        if timedelta(days=1) % self.interval:
            raise ValueError("interval must divide a day evenly")

    def next_after(self, dt):
        midnight = dt.replace(hour=0, minute=0, second=0, microsecond=0)
        steps = (dt - midnight) // self.interval + 1
        return midnight + steps * self.interval

    def __repr__(self):
        return f"Every({int(self.interval.total_seconds() // 60)}m)"


class Daily:
    """Run once a day at hour:minute."""

    def __init__(self, hour, minute=0):
        if not (0 <= hour <= 23 and 0 <= minute <= 59):
            raise ValueError("bad time of day")
        self.hour = hour
        self.minute = minute

    def next_after(self, dt):
        run = dt.replace(hour=self.hour, minute=self.minute, second=0, microsecond=0)
        if run <= dt:
            run += timedelta(days=1)
        return run

    def __repr__(self):
        return f"Daily({self.hour:02d}:{self.minute:02d})"


class Cron:
    """A classic 5-field cron expression: minute hour day-of-month month day-of-week.

    Items are *, N, A-B, */S and A-B/S, separated by commas. Day of week
    0 and 7 are Sunday. When both day fields are restricted, a day matches
    if either matches.
    """

    FIELDS = (("minute", 0, 59), ("hour", 0, 23), ("day of month", 1, 31),
              ("month", 1, 12), ("day of week", 0, 7))
    _ITEM = re.compile(r"(\*|(\d+)(?:-(\d+))?)(?:/(\d+))?")

    def __init__(self, expr):
        self.expr = expr
        parts = expr.split()
        if len(parts) != 5:
            raise ValueError(f"cron: expected 5 fields, got {len(parts)}: {expr!r}")
        sets = [self._field(p, *spec) for p, spec in zip(parts, self.FIELDS)]
        self.minutes, self.hours, self.days, self.months, dow = sets
        self.weekdays = {d % 7 for d in dow}  # 0 = Sunday
        self.days_restricted = parts[2] != "*"
        self.weekdays_restricted = parts[4] != "*"

    def _field(self, text, name, lo, hi):
        values = set()
        for item in text.split(","):
            m = self._ITEM.fullmatch(item)
            if not m:
                raise ValueError(f"cron: bad {name} field {text!r}")
            if m[1] == "*":
                start, end = lo, hi
            else:
                start = int(m[2])
                end = int(m[3]) if m[3] is not None else start
                if m[4] is not None and m[3] is None:
                    raise ValueError(f"cron: step needs a range in {name} field {text!r}")
            step = int(m[4]) if m[4] is not None else 1
            if step == 0:
                raise ValueError(f"cron: step 0 in {name} field {text!r}")
            if not (lo <= start <= hi and lo <= end <= hi):
                raise ValueError(f"cron: {name} out of range {lo}-{hi}: {text!r}")
            if start > end:
                raise ValueError(f"cron: empty range in {name} field {text!r}")
            values.update(range(start, end + 1, step))
        return values

    def _day_matches(self, d):
        dom = d.day in self.days
        dow = (d.weekday() + 1) % 7 in self.weekdays  # Python: Monday = 0
        if self.days_restricted and self.weekdays_restricted:
            return dom or dow
        return dom and dow

    def next_after(self, dt):
        t = dt.replace(second=0, microsecond=0) + timedelta(minutes=1)
        limit = dt + timedelta(days=5 * 366)
        while t <= limit:
            if t.month not in self.months or not self._day_matches(t):
                t = t.replace(hour=0, minute=0) + timedelta(days=1)
            elif t.hour not in self.hours:
                t = t.replace(minute=0) + timedelta(hours=1)
            elif t.minute not in self.minutes:
                t += timedelta(minutes=1)
            else:
                return t
        raise ValueError(f"cron: {self.expr!r} has no run within 5 years after {dt}")

    def __repr__(self):
        return f"Cron({self.expr!r})"


_EVERY = re.compile(r"every\s+(\d+)\s*([mh])")
_DAILY = re.compile(r"daily\s+(\d{1,2}):(\d{2})")
_CRON = re.compile(r"cron\s+(.+)")


def parse_schedule(text):
    """Parse a schedule as written in jobs.conf. Raises ValueError if it is not one."""
    text = text.strip()
    m = _EVERY.fullmatch(text)
    if m:
        n = int(m[1])
        return Every(minutes=n) if m[2] == "m" else Every(hours=n)
    m = _DAILY.fullmatch(text)
    if m:
        return Daily(int(m[1]), int(m[2]))
    m = _CRON.fullmatch(text)
    if m:
        return Cron(m[1])
    raise ValueError(f"not a schedule: {text!r}")
