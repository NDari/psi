import json
import os
import tempfile
import unittest
from datetime import date

from tracker import storage
from tracker.dates import parse_date
from tracker.filters import filter_tasks, group_by_project, sort_tasks
from tracker.model import Task

TODAY = date(2026, 3, 11)  # a Wednesday


def sample():
    return [
        Task(id=1, title="Buy milk", project="home", tags=["shop"], created=date(2026, 3, 1), due=date(2026, 3, 12)),
        Task(id=2, title="Fix bike", project="home", created=date(2026, 2, 1)),
        Task(id=3, title="Send invoice", project="work", created=date(2026, 3, 2), due=date(2026, 3, 9)),
        Task(id=4, title="archive mail", project=None, created=date(2026, 3, 3), status="done", done_on=date(2026, 3, 10)),
    ]


class StorageTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, "tasks.json")

    def tearDown(self):
        self.dir.cleanup()

    def test_missing_file_is_empty(self):
        self.assertEqual(storage.load(self.path), [])

    def test_round_trip(self):
        storage.save(self.path, sample())
        self.assertEqual(storage.load(self.path), sample())
        with open(self.path) as f:
            self.assertEqual(json.load(f)["version"], storage.FORMAT_VERSION)

    def test_version_1_list(self):
        with open(self.path, "w") as f:
            json.dump([{"id": 1, "title": "old"}], f)
        self.assertEqual([t.title for t in storage.load(self.path)], ["old"])

    def test_bad_json(self):
        with open(self.path, "w") as f:
            f.write("{nope")
        with self.assertRaises(storage.StorageError):
            storage.load(self.path)

    def test_next_id(self):
        self.assertEqual(storage.next_id(sample()), 5)
        self.assertEqual(storage.next_id([]), 1)


class FilterTest(unittest.TestCase):
    def test_status(self):
        self.assertEqual([t.id for t in filter_tasks(sample())], [1, 2, 3])
        self.assertEqual([t.id for t in filter_tasks(sample(), status="done")], [4])
        self.assertEqual(len(filter_tasks(sample(), status="all")), 4)

    def test_project_tag_overdue_text(self):
        self.assertEqual([t.id for t in filter_tasks(sample(), project="home")], [1, 2])
        self.assertEqual([t.id for t in filter_tasks(sample(), tag="#SHOP")], [1])
        self.assertEqual([t.id for t in filter_tasks(sample(), overdue=True, today=TODAY)], [3])
        self.assertEqual([t.id for t in filter_tasks(sample(), status=None, text="MAIL")], [4])

    def test_sort(self):
        self.assertEqual([t.id for t in sort_tasks(sample(), "due")], [3, 1, 2, 4])
        self.assertEqual([t.id for t in sort_tasks(sample(), "title")], [4, 1, 2, 3])
        self.assertEqual([t.id for t in sort_tasks(sample(), "created")], [2, 1, 3, 4])
        self.assertEqual([t.id for t in sort_tasks(sample(), "id", reverse=True)], [4, 3, 2, 1])
        with self.assertRaises(ValueError):
            sort_tasks(sample(), "colour")

    def test_group(self):
        self.assertEqual(list(group_by_project(sample())), ["home", "work", "(none)"])


class DatesTest(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(parse_date("tomorrow", TODAY), date(2026, 3, 12))
        self.assertEqual(parse_date("+2w", TODAY), date(2026, 3, 25))
        self.assertEqual(parse_date("wed", TODAY), date(2026, 3, 18))
        self.assertEqual(parse_date("Friday", TODAY), date(2026, 3, 13))
        self.assertEqual(parse_date("eow", TODAY), date(2026, 3, 15))
        self.assertEqual(parse_date("2026-12-01", TODAY), date(2026, 12, 1))
        with self.assertRaises(ValueError):
            parse_date("someday", TODAY)


if __name__ == "__main__":
    unittest.main()
