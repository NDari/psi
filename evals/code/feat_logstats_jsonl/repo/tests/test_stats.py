import unittest

from logstats import parse, report, stats

LINES = [
    '10.0.0.1 - - [10/Oct/2026:13:55:36 +0000] "GET / HTTP/1.1" 200 100',
    '10.0.0.2 - - [10/Oct/2026:13:59:00 +0000] "GET /x HTTP/1.1" 404 10',
    '10.0.0.1 - - [10/Oct/2026:14:01:00 +0000] "POST /x HTTP/1.1" 500 -',
]


class StatsTest(unittest.TestCase):
    def test_summary(self):
        s = stats.summarize(parse.parse_lines(LINES).entries)
        self.assertEqual((s.requests, s.bytes), (3, 110))
        self.assertEqual(s.by_path["/x"], 2)
        self.assertEqual(s.busiest_hour()[1], 2)

    def test_render(self):
        s = stats.summarize(parse.parse_lines(LINES).entries, skipped=1)
        out = report.render(s, top=1)
        self.assertIn("status: 2xx=1 4xx=1 5xx=1\n", out)
        self.assertIn("top paths:\n      2 /x\n", out)
        self.assertTrue(out.endswith("busiest hour: 2026-10-10 13:00 UTC (2 requests)\n"))


if __name__ == "__main__":
    unittest.main()
