import os
import tempfile
import unittest

from logsift.analyze import slow_requests
from logsift.parser import ParseError, parse_all, parse_line
from logsift.reader import read_lines
from logsift.report import render

LOG = """2024-05-01 10:00:03 INFO GET /api/users 123ms
2024-05-01 10:00:04 ERROR POST /api/orders 900ms

garbage line
2024-05-01 10:01:00 INFO GET /api/users 77ms
"""


class LogsiftTest(unittest.TestCase):
    def test_parse_line(self):
        e = parse_line("2024-05-01 10:00:03 INFO GET /api/users 123ms")
        self.assertEqual((e.level, e.method, e.path, e.ms), ("INFO", "GET", "/api/users", 123))
        with self.assertRaises(ParseError):
            parse_line("2024-05-01 INFO GET /x 1")

    def test_read_lines(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "a.log")
            with open(p, "w") as f:
                f.write(LOG)
            self.assertEqual(len(read_lines(p)), 4)

    def test_report(self):
        entries, skipped = parse_all(LOG.splitlines())
        self.assertEqual(skipped, 2)
        self.assertEqual([e.ms for e in slow_requests(entries)], [900])
        text = render(entries, skipped)
        self.assertIn("slow requests (>= 500 ms): 1", text)
        self.assertIn("  /api/orders avg 900 ms, max 900 ms, 1 calls", text)


if __name__ == "__main__":
    unittest.main()
