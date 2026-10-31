import unittest
from datetime import date

from tracker.model import Task, normalize_tag


class TaskTest(unittest.TestCase):
    def test_defaults(self):
        t = Task(id=1, title="Write report")
        self.assertEqual(t.status, "open")
        self.assertEqual(t.tags, [])
        self.assertIsNone(t.project)

    def test_title_whitespace_collapsed(self):
        self.assertEqual(Task(id=1, title="  a   b ").title, "a b")

    def test_rejects_bad_fields(self):
        for kwargs in (
            {"id": 0, "title": "x"},
            {"id": 1, "title": "   "},
            {"id": 1, "title": "x", "status": "waiting"},
            {"id": 1, "title": "x", "project": "has space"},
            {"id": 1, "title": "x", "done_on": date(2026, 1, 1)},
        ):
            with self.subTest(kwargs=kwargs):
                with self.assertRaises(ValueError):
                    Task(**kwargs)

    def test_tags_normalized(self):
        t = Task(id=1, title="x", tags=["#Shop", "shop", "home"])
        self.assertEqual(t.tags, ["shop", "home"])
        self.assertEqual(normalize_tag(" #A-b "), "a-b")

    def test_done_and_reopen(self):
        t = Task(id=1, title="x")
        t.mark_done(date(2026, 3, 1))
        self.assertEqual((t.status, t.done_on), ("done", date(2026, 3, 1)))
        with self.assertRaises(ValueError):
            t.mark_done(date(2026, 3, 2))
        t.reopen()
        self.assertTrue(t.is_open)
        self.assertIsNone(t.done_on)

    def test_overdue(self):
        t = Task(id=1, title="x", due=date(2026, 3, 1))
        self.assertTrue(t.is_overdue(date(2026, 3, 2)))
        self.assertFalse(t.is_overdue(date(2026, 3, 1)))

    def test_dict_round_trip(self):
        t = Task(id=4, title="x", project="p", tags=["a"], created=date(2026, 1, 2), due=date(2026, 2, 3))
        self.assertEqual(Task.from_dict(t.to_dict()), t)

    def test_from_dict_ignores_unknown_and_missing(self):
        t = Task.from_dict({"id": 2, "title": "y", "colour": "red"})
        self.assertEqual((t.id, t.title, t.status), (2, "y", "open"))


if __name__ == "__main__":
    unittest.main()
