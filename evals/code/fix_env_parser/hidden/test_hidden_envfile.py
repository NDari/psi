import os
import tempfile
import unittest

from envfile import load, parse


class TestHiddenEnvfile(unittest.TestCase):
    def test_equals_in_unquoted_value(self):
        env = parse("DATABASE_URL=postgres://app:pw@db:5432/app?sslmode=disable\n")
        self.assertEqual(env["DATABASE_URL"], "postgres://app:pw@db:5432/app?sslmode=disable")
        self.assertEqual(parse("TOKEN=abc==")["TOKEN"], "abc==")

    def test_equals_in_quoted_value(self):
        self.assertEqual(parse('Q="a=b=c"')["Q"], "a=b=c")
        self.assertEqual(parse("export OPTS='-Dx=1 -Dy=2'")["OPTS"], "-Dx=1 -Dy=2")

    def test_hash_in_double_quotes(self):
        self.assertEqual(parse('PW="s3cret # not a comment"')["PW"], "s3cret # not a comment")

    def test_hash_in_single_quotes(self):
        self.assertEqual(parse("COLOR='#ff0000 #00ff00'")["COLOR"], "#ff0000 #00ff00")

    def test_comment_after_quoted_value(self):
        env = parse('MSG="hi # there"   # greeting\nN=1')
        self.assertEqual(env, {"MSG": "hi # there", "N": "1"})

    def test_load_file(self):
        text = (
            "# settings\n"
            "export APP_ENV=production\n"
            "PORT=8080   # port\n"
            "URL=https://x.example/cb?a=1&b=2\n"
            'SECRET="p#ss = word"\n'
        )
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, ".env")
            with open(path, "w") as f:
                f.write(text)
            env = load(path)
        self.assertEqual(env, {
            "APP_ENV": "production",
            "PORT": "8080",
            "URL": "https://x.example/cb?a=1&b=2",
            "SECRET": "p#ss = word",
        })

    def test_existing_behaviour(self):
        env = parse(
            "\n# comment\nA=1 # one\nB = two  \nexport C=3\nD=\"x\\ny\"\nE='z'\n"
            "F=abc#def\nG=\nA=override\n"
        )
        self.assertEqual(env, {"A": "override", "B": "two", "C": "3", "D": "x\ny",
                               "E": "z", "F": "abc#def", "G": ""})
        for bad in ["NOEQUALS", "1KEY=x", "BAD KEY=x", 'Q="open']:
            with self.assertRaises(ValueError, msg=bad):
                parse(bad)


if __name__ == "__main__":
    unittest.main()
