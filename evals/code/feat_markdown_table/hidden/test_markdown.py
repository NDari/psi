import unittest

import report

ROWS = [
    {"region": "north", "total": "120", "note": "ok"},
    {"region": "south", "total": "75.5", "note": "late"},
]


class Markdown(unittest.TestCase):
    def test_basic_table(self):
        got = report.to_markdown(ROWS, ["region", "note"])
        self.assertEqual(got, "| region | note |\n| --- | --- |\n| north | ok |\n| south | late |\n")

    def test_numeric_column_right_aligned(self):
        got = report.to_markdown(ROWS, ["region", "total"])
        self.assertEqual(got.splitlines()[1], "| --- | ---: |")
        self.assertEqual(got.splitlines()[3], "| south | 75.5 |")

    def test_column_order_and_subset(self):
        got = report.to_markdown(ROWS, ["note", "region"])
        self.assertEqual(got.splitlines()[0], "| note | region |")
        self.assertEqual(got.splitlines()[2], "| ok | north |")

    def test_missing_and_none_cells(self):
        rows = [{"a": None, "n": "2"}, {"n": ""}, {"a": "x", "n": 7}]
        got = report.to_markdown(rows, ["a", "n"])
        self.assertEqual(got, "| a | n |\n| --- | ---: |\n|  | 2 |\n|  |  |\n| x | 7 |\n")

    def test_mixed_column_not_numeric(self):
        rows = [{"v": "1"}, {"v": "two"}, {"v": "3e2"}]
        self.assertEqual(report.to_markdown(rows, ["v"]).splitlines()[1], "| --- |")

    def test_pipe_escaped(self):
        rows = [{"a|b": "x|y|z"}]
        got = report.to_markdown(rows, ["a|b"])
        self.assertEqual(got, "| a\\|b |\n| --- |\n| x\\|y\\|z |\n")

    def test_no_rows(self):
        self.assertEqual(report.to_markdown([], ["a", "b"]), "| a | b |\n| --- | --- |\n")


class Existing(unittest.TestCase):
    def test_csv_and_text_unchanged(self):
        self.assertEqual(report.to_csv(ROWS, ["region", "total"]), "region,total\nnorth,120\nsouth,75.5\n")
        self.assertEqual(report.to_text(ROWS, ["region", "total"]),
                         "region  total\nnorth   120\nsouth   75.5\n")
