import unittest


def convert(text):
    from ini2json import convert as c
    return c(text)


class ConvertTest(unittest.TestCase):
    def test_scalars(self):
        got = convert("a = 12\nb = -7\nc = +3\nd = 2.50\ne = True\nf = false\ng = hello world\nh = 1.2.3\ni =\nj = 0.0.0.0\n")
        self.assertEqual(got, {"a": 12, "b": -7, "c": 3, "d": 2.5, "e": True, "f": False,
                               "g": "hello world", "h": "1.2.3", "i": "", "j": "0.0.0.0"})
        self.assertIs(type(got["d"]), float)
        self.assertIs(type(got["a"]), int)

    def test_comments_and_blank_lines(self):
        text = "; one\n# two\n\n   ; indented comment\n  # another\nx=1\n\n"
        self.assertEqual(convert(text), {"x": 1})

    def test_sections_and_nesting(self):
        text = "top = yes\n[server]\nport = 80\n[db.replica.east]\nhost = h1\n[ db.main ]\nhost = h0\n"
        self.assertEqual(convert(text), {"top": "yes", "server": {"port": 80},
                                         "db": {"replica": {"east": {"host": "h1"}}, "main": {"host": "h0"}}})

    def test_lists(self):
        got = convert("[x]\nl = a, b ,c\nm = 1,2.5, TRUE , word\n")
        self.assertEqual(got, {"x": {"l": ["a", "b", "c"], "m": [1, 2.5, True, "word"]}})

    def test_quoted_strings(self):
        got = convert('a = "x, y"\nb = "17"\nc = " padded "\nd = "true"\n')
        self.assertEqual(got, {"a": "x, y", "b": "17", "c": " padded ", "d": "true"})

    def test_repeats(self):
        got = convert("[s]\na = 1\nb = 2\n[t]\nz = 0\n[s]\na = 3\nc = 4\n")
        self.assertEqual(got, {"s": {"a": 3, "b": 2, "c": 4}, "t": {"z": 0}})
        self.assertEqual(list(got["s"]), ["a", "b", "c"])

    def test_error(self):
        with self.assertRaises(ValueError) as cm:
            convert("a = 1\n\n# c\njust words\n")
        self.assertEqual(str(cm.exception), "line 4: cannot parse")
        with self.assertRaises(ValueError) as cm:
            convert("[s]\n= 5\n")
        self.assertEqual(str(cm.exception), "line 2: cannot parse")


if __name__ == "__main__":
    unittest.main()
