import unittest
from datetime import datetime, timezone

from latency.loader import Record, Skipped, read_log
from latency.stats import by_endpoint, only_endpoint, percentile, summarize

LOG = "/hidden/gw.log"


def rec(path, ms, status=200):
    return Record(datetime(2026, 10, 21, tzinfo=timezone.utc), "GET", path, status, ms)


def expected(path=None):
    ms = []
    with open(LOG) as f:
        for line in f:
            parts = line.split()
            if len(parts) != 5 or not parts[4].endswith("ms") or not parts[4][:-2].isdigit():
                continue
            p = parts[2].split("?")[0].rstrip("/") or "/"
            if path is None or p == path:
                ms.append(int(parts[4][:-2]))
    ms.sort()
    return ms


class TestHiddenLatency(unittest.TestCase):
    def check(self, summary, ms):
        self.assertEqual(summary.count, len(ms))
        self.assertEqual(summary.p50, percentile(ms, 50))
        self.assertEqual(summary.p95, percentile(ms, 95))
        self.assertEqual(summary.max, ms[-1])

    def test_summarize_read_log(self):
        self.check(summarize(read_log(LOG)), expected())

    def test_summarize_one_endpoint(self):
        self.check(summarize(only_endpoint(read_log(LOG), "/health")), expected("/health"))

    def test_by_endpoint(self):
        result = by_endpoint(read_log(LOG))
        self.assertEqual(list(result), ["/api/login", "/api/orders", "/api/search", "/api/users", "/health"])
        for path, summary in result.items():
            self.check(summary, expected(path))

    def test_existing_behaviour(self):
        s = summarize([rec("/a", ms) for ms in (30, 10, 20, 40)] + [rec("/a", 100, 503)])
        self.assertEqual((s.count, s.mean, s.p50, s.p95, s.max, s.error_rate), (5, 40.0, 30, 100, 100, 0.2))
        self.assertIsNone(summarize([]))
        self.assertIsNone(summarize(only_endpoint(read_log(LOG), "/nope")))
        skipped = Skipped()
        self.assertEqual(sum(1 for _ in read_log(LOG, skipped)), len(expected()))
        self.assertEqual(skipped.count, 2)
        self.assertEqual(percentile(list(range(1, 21)), 95), 19)


if __name__ == "__main__":
    unittest.main()
