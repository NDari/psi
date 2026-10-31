import unittest
from datetime import datetime

from runner import Runner
from schedule import Daily, Every, parse_schedule


class ScheduleTest(unittest.TestCase):
    def test_every(self):
        e = Every(minutes=15)
        self.assertEqual(e.next_after(datetime(2026, 5, 1, 10, 7)), datetime(2026, 5, 1, 10, 15))
        self.assertEqual(e.next_after(datetime(2026, 5, 1, 10, 15)), datetime(2026, 5, 1, 10, 30))
        self.assertEqual(e.next_after(datetime(2026, 5, 1, 23, 50)), datetime(2026, 5, 2, 0, 0))

    def test_daily(self):
        d = Daily(6, 30)
        self.assertEqual(d.next_after(datetime(2026, 5, 1, 6, 30)), datetime(2026, 5, 2, 6, 30))

    def test_parse(self):
        self.assertEqual(repr(parse_schedule("every 2h")), "Every(120m)")
        with self.assertRaises(ValueError):
            parse_schedule("sometimes")

    def test_runner(self):
        now = [datetime(2026, 5, 1, 10, 0)]
        r = Runner(clock=lambda: now[0])
        hits = []
        r.add("a", Every(minutes=30), lambda: hits.append(now[0]))
        self.assertEqual(r.tick(), [])
        now[0] = datetime(2026, 5, 1, 10, 31)
        self.assertEqual(r.tick(), ["a"])
        self.assertEqual(r.jobs["a"].next_run, datetime(2026, 5, 1, 11, 0))


if __name__ == "__main__":
    unittest.main()
