import unittest
from datetime import datetime, timezone

from latency.loader import Record, parse_line
from latency.stats import by_endpoint, percentile, summarize

T = datetime(2026, 10, 20, tzinfo=timezone.utc)


def rec(path, ms, status=200):
    return Record(T, "GET", path, status, ms)


class TestStats(unittest.TestCase):
    def test_percentile_nearest_rank(self):
        values = list(range(1, 21))
        self.assertEqual(percentile(values, 50), 10)
        self.assertEqual(percentile(values, 95), 19)
        self.assertIsNone(percentile([], 50))

    def test_summarize(self):
        records = [rec("/a", ms) for ms in (30, 10, 20, 40)] + [rec("/a", 100, 503)]
        s = summarize(records)
        self.assertEqual((s.count, s.mean, s.p50, s.p95, s.max), (5, 40.0, 30, 100, 100))
        self.assertAlmostEqual(s.error_rate, 0.2)

    def test_summarize_empty(self):
        self.assertIsNone(summarize([]))

    def test_parse_line(self):
        r = parse_line("2026-10-20T10:00:01Z GET /api/users/?page=2 200 123ms")
        self.assertEqual((r.path, r.status, r.ms), ("/api/users", 200, 123))
        self.assertIsNone(parse_line("garbage"))


if __name__ == "__main__":
    unittest.main()
