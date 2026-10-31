"""Schedules for the job runner.

A schedule is any object with a `next_after(dt)` method that returns the
first run time strictly after the naive datetime `dt`. The runner only
uses that method, so new kinds of schedule fit in without other changes.

Schedules are written in jobs.conf as text and read by parse_schedule():

    every 15m          -> Every(minutes=15)
    every 2h           -> Every(hours=2)
    daily 06:30        -> Daily(6, 30)
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


_EVERY = re.compile(r"every\s+(\d+)\s*([mh])")
_DAILY = re.compile(r"daily\s+(\d{1,2}):(\d{2})")


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
    raise ValueError(f"not a schedule: {text!r}")
