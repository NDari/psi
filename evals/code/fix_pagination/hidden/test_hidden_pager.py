import unittest

import api
from pager import fetch_all, paginate

ITEMS = list(range(1, 46))


class TestHiddenPager(unittest.TestCase):
    def test_first_page_items(self):
        self.assertEqual(paginate(ITEMS, 1, 10).items, list(range(1, 11)))
        self.assertEqual(paginate(ITEMS, 2, 10).items, list(range(11, 21)))

    def test_last_partial_page(self):
        page = paginate(ITEMS, 5, 10)
        self.assertEqual(page.items, [41, 42, 43, 44, 45])
        self.assertFalse(page.has_next)
        self.assertTrue(page.has_prev)

    def test_total_pages_partial(self):
        self.assertEqual(paginate(ITEMS, 1, 10).total_pages, 5)
        self.assertEqual(paginate([1], 1, 20).total_pages, 1)
        self.assertEqual(paginate(list(range(41)), 1, 10).total_pages, 5)

    def test_has_next_before_last(self):
        self.assertTrue(paginate(ITEMS, 4, 10).has_next)
        self.assertEqual(paginate(ITEMS, 4, 10).to_dict()["next_page"], 5)

    def test_fetch_all(self):
        for per_page in (1, 7, 10, 20, 45, 50):
            got = fetch_all(lambda n: paginate(ITEMS, n, per_page))
            self.assertEqual(got, ITEMS, f"per_page={per_page}")

    def test_api_pages(self):
        status, body = api.list_users({"page": "3", "per_page": "20"})
        self.assertEqual(status, 200)
        self.assertEqual([u["id"] for u in body["items"]], [41, 42, 43, 44, 45])
        self.assertEqual(body["total_pages"], 3)
        self.assertIsNone(body["next_page"])
        self.assertEqual(body["prev_page"], 2)
        status, body = api.list_users({})
        self.assertEqual(body["items"][0], {"id": 1, "name": "user001"})

    def test_existing_behaviour(self):
        self.assertEqual(paginate(list(range(40)), 1, 10).total_pages, 4)
        self.assertEqual(paginate([], 1, 10).total_pages, 0)
        self.assertEqual(paginate([], 1, 10).items, [])
        past = paginate(ITEMS, 9, 10)
        self.assertEqual(past.items, [])
        self.assertFalse(past.has_next)
        for page, per_page in ((0, 10), (1, 0), (-1, 5)):
            with self.assertRaises(ValueError):
                paginate(ITEMS, page, per_page)
        self.assertEqual(api.list_users({"per_page": "nope"})[0], 400)


if __name__ == "__main__":
    unittest.main()
