import unittest

import textkit
from textkit import align, render, slugify, table, top_words, wrap


class NormalizeTest(unittest.TestCase):
    def test_slugify(self):
        self.assertEqual(slugify("Héllo, Wörld!"), "hello-world")
        self.assertEqual(slugify("a b c d", max_length=4), "a-b")

    def test_cases(self):
        self.assertEqual(textkit.snake_case("parseHTTPResponse"), "parse_http_response")
        self.assertEqual(textkit.camel_case("total amount net"), "totalAmountNet")
        self.assertEqual(textkit.title_case("the lord of the rings"), "The Lord of the Rings")


class LayoutTest(unittest.TestCase):
    def test_wrap(self):
        self.assertEqual(wrap("the quick brown fox", 10), ["the quick", "brown fox"])

    def test_align(self):
        self.assertEqual(align("ab", 5, "center", "."), ".ab..")

    def test_table(self):
        got = table([["a", 1], ["bb", 22]], headers=["k", "v"], aligns="lr")
        self.assertEqual(got, "k    v\n--  --\na    1\nbb  22")


class StatsTest(unittest.TestCase):
    def test_counts(self):
        self.assertEqual(textkit.word_count("Don't panic, it's fine."), 4)
        self.assertEqual(textkit.sentence_count("One. Two! Three"), 3)

    def test_top_words(self):
        text = "Apple banana apple cherry banana apple the the the"
        self.assertEqual(top_words(text, 2), [("apple", 3), ("banana", 2)])


class TemplateTest(unittest.TestCase):
    def test_render(self):
        self.assertEqual(render("Hi {{ name | upper }}!", {"name": "ada"}), "Hi ADA!")

    def test_custom_filter(self):
        @textkit.register_filter("twice")
        def twice(value):
            return str(value) * 2

        self.assertEqual(render("{{ x | twice }}", x="ab"), "abab")


if __name__ == "__main__":
    unittest.main()
