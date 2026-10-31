import unittest

from api import list_users
from pager import paginate


class TestPager(unittest.TestCase):
    def test_total_pages_even(self):
        self.assertEqual(paginate(list(range(40)), 1, 10).total_pages, 4)

    def test_first_page(self):
        self.assertEqual(paginate(list("abcdefg"), 1, 3).items, ["a", "b", "c"])

    def test_bad_page(self):
        with self.assertRaises(ValueError):
            paginate([1, 2, 3], 0, 10)

    def test_api_bad_param(self):
        status, body = list_users({"page": "x"})
        self.assertEqual(status, 400)


if __name__ == "__main__":
    unittest.main()
