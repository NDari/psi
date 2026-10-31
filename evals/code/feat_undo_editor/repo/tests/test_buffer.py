import unittest

from buffer import TextBuffer


class BufferTest(unittest.TestCase):
    def test_insert_at_cursor(self):
        b = TextBuffer("held")
        b.move(3)
        b.insert("l")
        self.assertEqual((b.text, b.cursor), ("helld", 4))

    def test_delete(self):
        b = TextBuffer("abc")
        self.assertEqual(b.delete(5), 3)
        self.assertEqual((b.text, b.cursor), ("", 0))

    def test_replace_all(self):
        b = TextBuffer("a-b-c")
        self.assertEqual(b.replace_all("-", "+"), 2)
        self.assertEqual(b.text, "a+b+c")


if __name__ == "__main__":
    unittest.main()
