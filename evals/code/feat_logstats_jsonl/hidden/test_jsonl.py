import json
import unittest
from datetime import datetime, timedelta, timezone

from logstats import parse

BASE = {"host": "10.1.1.1", "time": "2026-10-11T08:00:05-03:00", "method": "PATCH",
        "path": "/v1/users/9?fields=name", "status": 202, "bytes": 77}


def line(**changes):
    obj = dict(BASE)
    for k, v in changes.items():
        if v is KeyError:
            del obj[k]
        else:
            obj[k] = v
    return json.dumps(obj) + "\n"


class ParseJsonl(unittest.TestCase):
    def test_valid_line(self):
        e = parse.parse_jsonl_line(line())
        self.assertEqual(e.host, "10.1.1.1")
        self.assertEqual(e.time, datetime(2026, 10, 11, 11, 0, 5, tzinfo=timezone.utc))
        self.assertEqual(e.time.utcoffset(), timedelta(hours=-3))
        self.assertEqual((e.method, e.path, e.status, e.size), ("PATCH", "/v1/users/9", 202, 77))
        self.assertEqual(e.status_class, "2xx")

    def test_bytes_null_or_absent(self):
        self.assertEqual(parse.parse_jsonl_line(line(bytes=None)).size, 0)
        self.assertEqual(parse.parse_jsonl_line(line(bytes=KeyError)).size, 0)

    def test_malformed(self):
        bad = [
            "not json\n",
            "[1, 2]\n",
            '"a string"\n',
            line(host=KeyError),
            line(path=KeyError),
            line(status="202"),
            line(status=None),
            line(time="2026-10-11T08:00:05"),
            line(time="soon"),
            line(bytes="77"),
        ]
        for text in bad:
            with self.subTest(text=text):
                self.assertIsNone(parse.parse_jsonl_line(text))
