# reminders

Daily reminders at a fixed local time, in any IANA time zone.

    python3 -m reminders list --at 09:00 --tz Europe/Berlin --from 2026-10-23T12:00 --count 4

prints the next COUNT times (strictly after --from, which is a local time in
--tz) at which the clock in that zone shows --at, one per line, with the
time left from --from:

    2026-10-24 09:00 CEST (in 21 h 0 min)

The countdown is the real time that passes, so it accounts for clock
changes.

Python API (reminders.schedule):

- `next_daily(at, tz, after)`: the next aware datetime, in zone `tz`,
  strictly after the aware datetime `after`, at which the local clock shows
  `at` (a datetime.time).
- `upcoming(at, tz, after, count)`: the next `count` such datetimes.
- `hours_between(start, end)`: real elapsed hours between two aware datetimes.
- `format_countdown(start, end)`: e.g. "in 25 h 0 min".
