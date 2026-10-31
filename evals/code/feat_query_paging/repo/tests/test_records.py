import unittest

from records import Query

PEOPLE = [
    {"name": "Ada", "city": "Oslo", "age": 36},
    {"name": "Bo", "city": "Rome", "age": 17},
    {"name": "Cy", "city": "Oslo", "age": 52},
    {"name": "Di", "age": 41},
]


class QueryTest(unittest.TestCase):
    def test_where(self):
        self.assertEqual([r["name"] for r in Query(PEOPLE).where(city="Oslo").all()], ["Ada", "Cy"])
        self.assertEqual(Query(PEOPLE).where(city=["Rome", "Oslo"]).count(), 3)

    def test_where_fn_and_select(self):
        q = Query(PEOPLE).where_fn(lambda r: r["age"] >= 40).select("name", "city")
        self.assertEqual(q.all(), [{"name": "Cy", "city": "Oslo"}, {"name": "Di", "city": None}])

    def test_builders_do_not_change_base(self):
        base = Query(PEOPLE)
        base.where(city="Oslo")
        self.assertEqual(base.count(), 4)

    def test_first(self):
        self.assertEqual(Query(PEOPLE).where(city="Paris").first(), None)
        self.assertEqual(Query(PEOPLE).first()["name"], "Ada")


if __name__ == "__main__":
    unittest.main()
