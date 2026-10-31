import unittest
from datetime import datetime, time, timedelta, timezone
from zoneinfo import ZoneInfo

from reminders.schedule import format_countdown, hours_between, next_daily, upcoming

BER = ZoneInfo("Europe/Berlin")
NY = ZoneInfo("America/New_York")
UTC = timezone.utc


def wall(dt):
    return (dt.year, dt.month, dt.day, dt.hour, dt.minute)


class TestHiddenReminders(unittest.TestCase):
    def test_next_daily_across_fall_back(self):
        got = next_daily(time(9, 0), "Europe/Berlin", datetime(2026, 10, 24, 10, 0, tzinfo=BER))
        self.assertEqual(wall(got.astimezone(BER)), (2026, 10, 25, 9, 0))
        self.assertEqual(got.astimezone(UTC), datetime(2026, 10, 25, 8, 0, tzinfo=UTC))

    def test_next_daily_across_spring_forward(self):
        after = datetime(2026, 3, 28, 9, 0, tzinfo=BER)  # exactly at the time: next day
        got = next_daily(time(9, 0), "Europe/Berlin", after)
        self.assertEqual(wall(got.astimezone(BER)), (2026, 3, 29, 9, 0))
        self.assertEqual(got.astimezone(UTC), datetime(2026, 3, 29, 7, 0, tzinfo=UTC))

    def test_upcoming_across_fall_back(self):
        got = upcoming(time(9, 0), "Europe/Berlin", datetime(2026, 10, 23, 12, 0, tzinfo=BER), 4)
        self.assertEqual([wall(t.astimezone(BER)) for t in got],
                         [(2026, 10, d, 9, 0) for d in (24, 25, 26, 27)])

    def test_upcoming_new_york(self):
        after = datetime(2026, 10, 30, 12, 0, tzinfo=UTC)
        got = upcoming(time(7, 30), "America/New_York", after, 4)
        self.assertEqual([wall(t.astimezone(NY)) for t in got],
                         [(2026, 10, 31, 7, 30), (2026, 11, 1, 7, 30), (2026, 11, 2, 7, 30), (2026, 11, 3, 7, 30)])
        self.assertEqual(got[1].astimezone(UTC), datetime(2026, 11, 1, 12, 30, tzinfo=UTC))

    def test_hours_between_dst(self):
        self.assertEqual(hours_between(datetime(2026, 10, 24, 9, tzinfo=BER), datetime(2026, 10, 25, 9, tzinfo=BER)), 25.0)
        self.assertEqual(hours_between(datetime(2026, 3, 28, 9, tzinfo=BER), datetime(2026, 3, 29, 9, tzinfo=BER)), 23.0)
        self.assertEqual(hours_between(datetime(2026, 10, 31, 23, tzinfo=NY), datetime(2026, 11, 1, 3, tzinfo=NY)), 5.0)

    def test_countdown_dst(self):
        self.assertEqual(format_countdown(datetime(2026, 10, 24, 9, tzinfo=BER), datetime(2026, 10, 25, 9, 30, tzinfo=BER)),
                         "in 25 h 30 min")
        self.assertEqual(format_countdown(datetime(2026, 3, 28, 12, tzinfo=BER), datetime(2026, 3, 29, 9, tzinfo=BER)),
                         "in 20 h 0 min")

    def test_existing_behaviour(self):
        july = datetime(2026, 7, 1, 7, 0, tzinfo=BER)
        self.assertEqual(wall(next_daily(time(9, 0), "Europe/Berlin", july)), (2026, 7, 1, 9, 0))
        self.assertEqual(wall(next_daily(time(6, 15), "Europe/Berlin", july)), (2026, 7, 2, 6, 15))
        got = upcoming(time(18, 0), "UTC", datetime(2026, 7, 1, 20, 0, tzinfo=UTC), 3)
        self.assertEqual([wall(t) for t in got], [(2026, 7, d, 18, 0) for d in (2, 3, 4)])
        self.assertEqual(upcoming(time(9, 0), "Europe/Berlin", july, 0), [])
        self.assertEqual(hours_between(july, july + timedelta(days=1)), 24.0)
        self.assertEqual(format_countdown(july, july + timedelta(hours=2, minutes=5)), "in 2 h 5 min")
        with self.assertRaises(ValueError):
            next_daily(time(9, 0), "Europe/Berlin", datetime(2026, 7, 1))


if __name__ == "__main__":
    unittest.main()
