import unittest

import records
from records import Query

ROWS = [
    {"id": 1, "team": "red", "score": 7},
    {"id": 2, "team": "blue", "score": None},
    {"id": 3, "team": "red", "score": 9},
    {"id": 4, "team": "blue", "score": 7},
    {"id": 5, "score": 3},
    {"id": 6, "team": "red", "score": 7},
    {"id": 7, "team": "green"},
]


def ids(rows):
    return [r["id"] for r in rows]


class OrderBy(unittest.TestCase):
    def test_ascending_none_last_stable(self):
        self.assertEqual(ids(Query(ROWS).order_by("score").all()), [5, 1, 4, 6, 3, 2, 7])

    def test_descending_none_still_last(self):
        self.assertEqual(ids(Query(ROWS).order_by("-score").all()), [3, 1, 4, 6, 5, 2, 7])

    def test_several_fields(self):
        self.assertEqual(ids(Query(ROWS).order_by("team", "-id").all()), [4, 2, 7, 6, 3, 1, 5])
        self.assertEqual(ids(Query(ROWS).order_by("-score", "team").all()), [3, 4, 1, 6, 5, 2, 7])

    def test_order_by_replaces_and_keeps_base(self):
        base = Query(ROWS).where(team="red")
        q = base.order_by("score").order_by("-id")
        self.assertEqual(ids(q.all()), [6, 3, 1])
        self.assertEqual(ids(base.all()), [1, 3, 6])
        self.assertEqual(q.select("id").first(), {"id": 6})


class LimitOffset(unittest.TestCase):
    def test_limit_offset(self):
        q = Query(ROWS).order_by("-id")
        self.assertEqual(ids(q.limit(3).all()), [7, 6, 5])
        self.assertEqual(ids(q.offset(2).limit(2).all()), [5, 4])
        self.assertEqual(ids(q.limit(2).offset(2).all()), [5, 4])
        self.assertEqual(ids(q.offset(10).all()), [])
        self.assertEqual(q.limit(0).all(), [])
        self.assertEqual(q.offset(5).limit(1).count(), 7)
        self.assertEqual(ids(q.all()), [7, 6, 5, 4, 3, 2, 1])

    def test_negative_rejected(self):
        with self.assertRaises(ValueError):
            Query(ROWS).limit(-1)
        with self.assertRaises(ValueError):
            Query(ROWS).offset(-1)


class Paging(unittest.TestCase):
    def test_page_fields(self):
        p = Query(ROWS).where_fn(lambda r: r["id"] != 7).order_by("id").select("id").page(2, 4)
        self.assertIsInstance(p, records.Page)
        self.assertEqual(p, records.Page([{"id": 5}, {"id": 6}], 2, 4, 6, 2))
        self.assertEqual(p.items, [{"id": 5}, {"id": 6}])
        self.assertEqual((p.number, p.size, p.total, p.pages), (2, 4, 6, 2))

    def test_page_ignores_limit_offset_and_past_end(self):
        q = Query(ROWS).order_by("id").offset(3).limit(1)
        p = q.page(1, 3)
        self.assertEqual((ids(p.items), p.total, p.pages), ([1, 2, 3], 7, 3))
        p = q.page(4, 3)
        self.assertEqual((p.items, p.total, p.pages), ([], 7, 3))
        p = Query([]).page(1, 10)
        self.assertEqual((p.items, p.total, p.pages), ([], 0, 0))

    def test_page_bad_arguments(self):
        for number, size in ((0, 5), (1, 0), (-1, 3)):
            with self.assertRaises(ValueError):
                Query(ROWS).page(number, size)


class Existing(unittest.TestCase):
    def test_where_select_count(self):
        q = Query(ROWS).where(team=["red", "green"])
        self.assertEqual(q.count(), 4)
        self.assertEqual(q.select("id", "score").first(), {"id": 1, "score": 7})
