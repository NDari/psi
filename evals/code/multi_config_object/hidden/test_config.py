import dataclasses
import datetime
import json
import os
import pathlib
import tempfile
import unittest

LINE = "2024-05-01 10:00:03 INFO GET /api/users 123ms"


class ConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def write(self, name, text):
        p = os.path.join(self.tmp.name, name)
        with open(p, "w") as f:
            f.write(text)
        return p

    def test_config_defaults(self):
        from logsift.config import Config
        self.assertTrue(dataclasses.is_dataclass(Config))
        c = Config()
        self.assertEqual((c.max_line_length, c.time_format, c.slow_ms, c.top_n),
                         (400, "%Y-%m-%d %H:%M:%S", 500, 5))
        self.assertEqual(Config(slow_ms=50).slow_ms, 50)

    def test_load_config(self):
        from logsift.config import load_config
        c = load_config(self.write("c.json", json.dumps({"slow_ms": 250, "top_n": 3})))
        self.assertEqual((c.max_line_length, c.time_format, c.slow_ms, c.top_n),
                         (400, "%Y-%m-%d %H:%M:%S", 250, 3))
        c = load_config(self.write("e.json", "{}"))
        self.assertEqual(c.slow_ms, 500)
        with self.assertRaises(ValueError):
            load_config(self.write("bad.json", json.dumps({"slow_ms": 1, "colour": "red"})))

    def test_read_lines_config(self):
        from logsift.config import Config
        from logsift.reader import read_lines
        p = self.write("a.log", "abcdefghijklmnop\n\nshort\n" + "z" * 450 + "\n")
        self.assertEqual(read_lines(p, config=Config(max_line_length=10)), ["abcdefghij", "short", "z" * 10])
        self.assertEqual(read_lines(p)[2], "z" * 400)

    def test_parse_config(self):
        from logsift.config import Config
        from logsift.parser import ParseError, parse_all, parse_line
        c = Config(time_format="%d.%m.%Y %H:%M")
        e = parse_line("03.02.2024 07:30 WARN PUT /x 9ms", config=c)
        self.assertEqual((e.time, e.ms), (datetime.datetime(2024, 2, 3, 7, 30), 9))
        with self.assertRaises(ParseError):
            parse_line(LINE, config=c)
        entries, skipped = parse_all(["03.02.2024 07:30 WARN PUT /x 9ms", LINE], config=c)
        self.assertEqual((len(entries), skipped), (1, 1))

    def test_slow_requests_config(self):
        from logsift.analyze import slow_requests
        from logsift.config import Config
        from logsift.parser import parse_line
        es = [parse_line(LINE.replace("123ms", f"{n}ms")) for n in (99, 100, 450, 501)]
        self.assertEqual([e.ms for e in slow_requests(es, config=Config(slow_ms=100))], [501, 450, 100])
        self.assertEqual([e.ms for e in slow_requests(es, config=Config(slow_ms=1000))], [])

    def test_render_config(self):
        from logsift.config import Config
        from logsift.parser import parse_line
        from logsift.report import render
        es = [parse_line(f"2024-05-01 10:00:0{i} INFO GET /p{i} {i * 100}ms") for i in range(1, 5)]
        text = render(es, 0, config=Config(time_format="%H:%M:%S", slow_ms=200, top_n=2))
        want = ("from 10:00:01 to 10:00:04\nrequests 4, errors 0, skipped lines 0\n"
                "slow requests (>= 200 ms): 3\nslowest paths:\n"
                "  /p4 avg 400 ms, max 400 ms, 1 calls\n  /p3 avg 300 ms, max 300 ms, 1 calls\n")
        self.assertEqual(text, want)

    def test_defaults_unchanged(self):
        from logsift.analyze import slow_requests
        from logsift.parser import parse_line
        from logsift.report import render
        es = [parse_line(f"2024-05-01 10:00:0{i} INFO GET /p{i} {i * 150}ms") for i in range(1, 8)]
        self.assertEqual([e.ms for e in slow_requests(es)], [1050, 900, 750, 600])
        text = render(es)
        self.assertIn("slow requests (>= 500 ms): 4", text)
        self.assertEqual(text.count(" calls"), 5)

    def test_no_hard_coded_values(self):
        for p in pathlib.Path("logsift").glob("*.py"):
            if p.name == "config.py":
                continue
            src = p.read_text()
            for literal in ("400", "500", "%H:%M:%S"):
                self.assertNotIn(literal, src, f"{p} still has {literal}")


if __name__ == "__main__":
    unittest.main()
