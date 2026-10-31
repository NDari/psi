import unittest

from json2ini import to_ini


class Json2IniTest(unittest.TestCase):
    def test_nested(self):
        data = {"a": 1, "s": {"b": True, "t": {"c": [1, "x"]}}}
        self.assertEqual(to_ini(data), "a = 1\n\n[s]\nb = true\n\n[s.t]\nc = 1, x\n")


if __name__ == "__main__":
    unittest.main()
