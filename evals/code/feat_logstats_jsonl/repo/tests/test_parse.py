import unittest
from datetime import datetime, timedelta, timezone

from logstats import parse

LINE = '10.0.0.1 - frank [10/Oct/2026:13:55:36 +0200] "GET /a.gif?x=1 HTTP/1.1" 200 2326'


class ParseTest(unittest.TestCase):
    def test_clf_line(self):
        e = parse.parse_clf_line(LINE)
        self.assertEqual(e.host, "10.0.0.1")
        self.assertEqual(e.time, datetime(2026, 10, 10, 13, 55, 36, tzinfo=timezone(timedelta(hours=2))))
        self.assertEqual((e.method, e.path, e.status, e.size), ("GET", "/a.gif", 200, 2326))

    def test_dash_size(self):
        self.assertEqual(parse.parse_clf_line(LINE.replace("2326", "-")).size, 0)

    def test_malformed(self):
        self.assertIsNone(parse.parse_clf_line("garbage"))

    def test_parse_lines_counts_skipped(self):
        r = parse.parse_lines([LINE, "", "junk", LINE])
        self.assertEqual((len(r.entries), r.skipped), (2, 1))


if __name__ == "__main__":
    unittest.main()
