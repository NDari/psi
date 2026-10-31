import unittest

import buffer


def make(text="", **kw):
    return buffer.TextBuffer(text, **kw)


def state(b):
    return (b.text, b.cursor)


class Undo(unittest.TestCase):
    def test_undo_insert_restores_text_and_cursor(self):
        b = make("hello world")
        b.move(5)
        b.insert(",")
        b.move(0)
        self.assertTrue(b.undo())
        self.assertEqual(state(b), ("hello world", 5))

    def test_undo_redo_sequence(self):
        b = make("ab")
        b.insert("c")
        b.delete(2)
        b.insert("XY")
        self.assertEqual(state(b), ("aXY", 3))
        self.assertTrue(b.undo())
        self.assertEqual(state(b), ("a", 1))
        self.assertTrue(b.undo())
        self.assertEqual(state(b), ("abc", 3))
        self.assertTrue(b.redo())
        self.assertEqual(state(b), ("a", 1))
        self.assertTrue(b.redo())
        self.assertEqual(state(b), ("aXY", 3))
        self.assertFalse(b.redo())
        self.assertEqual(state(b), ("aXY", 3))

    def test_redo_restores_after_state_even_if_cursor_moved(self):
        b = make("xyz")
        b.move(1)
        b.delete(1)
        b.undo()
        b.move(3)
        b.redo()
        self.assertEqual(state(b), ("yz", 0))

    def test_nothing_to_undo(self):
        b = make("abc")
        b.move(1)
        self.assertFalse(b.undo())
        self.assertFalse(b.redo())
        self.assertEqual(state(b), ("abc", 1))
        self.assertIs(b.can_undo, False)
        self.assertIs(b.can_redo, False)

    def test_new_edit_clears_redo(self):
        b = make("one")
        b.insert(" two")
        b.undo()
        self.assertIs(b.can_redo, True)
        b.insert("!")
        self.assertIs(b.can_redo, False)
        self.assertFalse(b.redo())
        self.assertEqual(b.text, "one!")
        b.undo()
        self.assertEqual(b.text, "one")
        self.assertIs(b.can_undo, False)

    def test_noop_edits_add_no_step(self):
        b = make("abc")
        b.insert("Z")
        b.undo()
        b.insert("")
        b.move(0)
        b.delete(3)
        b.delete(0)
        b.replace_all("q", "r")
        self.assertIs(b.can_undo, False)
        self.assertIs(b.can_redo, True)
        b.redo()
        self.assertEqual(state(b), ("abcZ", 4))

    def test_replace_all_undo(self):
        b = make("a.b.c.d")
        b.move(6)
        self.assertEqual(b.replace_all(".", ""), 3)
        self.assertEqual(state(b), ("abcd", 4))
        b.undo()
        self.assertEqual(state(b), ("a.b.c.d", 6))

    def test_history_limit(self):
        b = make("", history_limit=2)
        for ch in "abcd":
            b.insert(ch)
        self.assertTrue(b.undo())
        self.assertTrue(b.undo())
        self.assertFalse(b.undo())
        self.assertEqual(state(b), ("ab", 2))
        for n in (0, -1):
            with self.assertRaises(ValueError):
                make("x", history_limit=n)


class Existing(unittest.TestCase):
    def test_edits(self):
        b = make("abcdef")
        b.move(99)
        self.assertEqual(b.cursor, 6)
        b.move(2)
        self.assertEqual(b.delete(5), 2)
        b.insert("--")
        self.assertEqual(state(b), ("--cdef", 2))
        with self.assertRaises(ValueError):
            b.replace_all("", "x")
