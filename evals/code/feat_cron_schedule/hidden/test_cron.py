import unittest
from datetime import datetime as D

import schedule


def nxt(expr, *after):
    return schedule.Cron(expr).next_after(D(*after))


class Cron(unittest.TestCase):
    def test_steps_and_strictly_after(self):
        self.assertEqual(nxt("*/15 * * * *", 2026, 10, 28, 10, 7), D(2026, 10, 28, 10, 15))
        self.assertEqual(nxt("*/15 * * * *", 2026, 10, 28, 10, 15), D(2026, 10, 28, 10, 30))
        self.assertEqual(nxt("*/15 * * * *", 2026, 10, 28, 10, 14, 59, 500000), D(2026, 10, 28, 10, 15))
        self.assertEqual(nxt("*/15 * * * *", 2026, 10, 28, 23, 50), D(2026, 10, 29, 0, 0))

    def test_ranges_lists_and_range_steps(self):
        self.assertEqual(nxt("0 9-17/4 * * 1-5", 2026, 10, 28, 9, 30), D(2026, 10, 28, 13, 0))
        self.assertEqual(nxt("0 9-17/4 * * 1-5", 2026, 10, 30, 17, 0), D(2026, 11, 2, 9, 0))
        self.assertEqual(nxt("5,35 */6 * * *", 2026, 10, 28, 6, 40), D(2026, 10, 28, 12, 5))
        self.assertEqual(nxt("10-12,50 3 * * *", 2026, 10, 28, 3, 12), D(2026, 10, 28, 3, 50))

    def test_one_day_field_restricted_needs_both(self):
        self.assertEqual(nxt("0 12 * * 5", 2026, 10, 28, 0, 0), D(2026, 10, 30, 12, 0))
        self.assertEqual(nxt("0 12 13 * *", 2026, 10, 28, 0, 0), D(2026, 11, 13, 12, 0))

    def test_both_day_fields_restricted_means_either(self):
        self.assertEqual(nxt("30 8 1,15 * 0", 2026, 10, 12, 9, 0), D(2026, 10, 15, 8, 30))
        self.assertEqual(nxt("30 8 1,15 * 0", 2026, 10, 15, 9, 0), D(2026, 10, 18, 8, 30))
        self.assertEqual(nxt("0 0 13 * 5", 2026, 10, 28, 0, 0), D(2026, 10, 30, 0, 0))

    def test_sunday_as_seven(self):
        self.assertEqual(nxt("0 6 * * 7", 2026, 10, 28, 0, 0), D(2026, 11, 1, 6, 0))
        self.assertEqual(nxt("0 6 * * 6-7", 2026, 10, 31, 7, 0), D(2026, 11, 1, 6, 0))
        self.assertEqual(nxt("0 6 * * 0", 2026, 10, 28, 0, 0), D(2026, 11, 1, 6, 0))

    def test_month_and_year_rollover(self):
        self.assertEqual(nxt("0 0 1 1 *", 2026, 10, 28, 0, 0), D(2027, 1, 1, 0, 0))
        self.assertEqual(nxt("15 10 * 2 *", 2026, 10, 28, 0, 0), D(2027, 2, 1, 10, 15))
        self.assertEqual(nxt("59 23 31 12 *", 2026, 12, 31, 23, 59), D(2027, 12, 31, 23, 59))

    def test_leap_day(self):
        self.assertEqual(nxt("0 0 29 2 *", 2026, 3, 1, 0, 0), D(2028, 2, 29, 0, 0))

    def test_invalid_expressions(self):
        bad = ["* * * *", "* * * * * *", "60 * * * *", "* 24 * * *", "* * 0 * *",
               "* * 32 * *", "* * * 13 *", "* * * 0 *", "* * * * 8", "*/0 * * * *",
               "5-1 * * * *", "a * * * *", "1,,2 * * * *", "-1 * * * *",
               "* * * JAN *", "", "1-2-3 * * * *", "*/ * * * *"]
        for expr in bad:
            with self.subTest(expr=expr):
                with self.assertRaises(ValueError):
                    schedule.Cron(expr)

    def test_impossible_raises(self):
        for expr in ("0 0 30 2 *", "0 0 31 4,6,9,11 *"):
            with self.subTest(expr=expr):
                with self.assertRaises(ValueError):
                    nxt(expr, 2026, 10, 28, 0, 0)

    def test_parse_schedule_and_runner(self):
        from runner import Runner
        s = schedule.parse_schedule("cron 0 9 * * 1-5")
        self.assertIsInstance(s, schedule.Cron)
        self.assertEqual(s.next_after(D(2026, 10, 31, 12, 0)), D(2026, 11, 2, 9, 0))
        now = [D(2026, 10, 28, 8, 58)]
        r = Runner(clock=lambda: now[0])
        r.add("report", schedule.parse_schedule("cron */2 9 * * *"), lambda: None)
        self.assertEqual(r.jobs["report"].next_run, D(2026, 10, 28, 9, 0))
        now[0] = D(2026, 10, 28, 9, 1)
        self.assertEqual(r.tick(), ["report"])
        self.assertEqual(r.jobs["report"].next_run, D(2026, 10, 28, 9, 2))


class Existing(unittest.TestCase):
    def test_other_schedules(self):
        self.assertEqual(schedule.parse_schedule("every 20m").next_after(D(2026, 1, 1, 0, 59)),
                         D(2026, 1, 1, 1, 0))
        self.assertEqual(schedule.parse_schedule("daily 06:30").next_after(D(2026, 1, 1, 7, 0)),
                         D(2026, 1, 2, 6, 30))
        with self.assertRaises(ValueError):
            schedule.parse_schedule("cronjob")
