"""The text model behind the note editor: a string plus a cursor.

The cursor is an index into the text, 0 <= cursor <= len(text). Edits
happen at the cursor, like typing. Every edit that changes the text can be
undone and redone.
"""
from collections import deque


class TextBuffer:
    def __init__(self, text="", history_limit=100):
        if history_limit < 1:
            raise ValueError("history_limit must be at least 1")
        self._text = text
        self._cursor = len(text)
        # each step: (text, cursor) before the edit and (text, cursor) after it
        self._undo = deque(maxlen=history_limit)
        self._redo = []

    def _record(self, before):
        if self._text == before[0]:
            return
        self._undo.append((before, (self._text, self._cursor)))
        self._redo.clear()

    @property
    def can_undo(self):
        return bool(self._undo)

    @property
    def can_redo(self):
        return bool(self._redo)

    def undo(self):
        """Revert the most recent edit. Returns False when there is none."""
        if not self._undo:
            return False
        step = self._undo.pop()
        self._text, self._cursor = step[0]
        self._redo.append(step)
        return True

    def redo(self):
        """Re-apply the most recently undone edit. Returns False when there is none."""
        if not self._redo:
            return False
        step = self._redo.pop()
        self._text, self._cursor = step[1]
        self._undo.append(step)
        return True

    @property
    def text(self):
        return self._text

    @property
    def cursor(self):
        return self._cursor

    def move(self, pos):
        """Put the cursor at pos, clamped to the text."""
        self._cursor = max(0, min(pos, len(self._text)))

    def insert(self, s):
        """Insert s at the cursor and move the cursor past it."""
        before = (self._text, self._cursor)
        c = self._cursor
        self._text = self._text[:c] + s + self._text[c:]
        self._cursor = c + len(s)
        self._record(before)

    def delete(self, n=1):
        """Delete up to n characters before the cursor (like backspace).

        Returns the number of characters deleted.
        """
        if n < 0:
            raise ValueError("n must not be negative")
        before = (self._text, self._cursor)
        c = self._cursor
        n = min(n, c)
        self._text = self._text[:c - n] + self._text[c:]
        self._cursor = c - n
        self._record(before)
        return n

    def replace_all(self, old, new):
        """Replace every occurrence of old with new. Returns the count.

        The cursor stays at the same index, clamped to the new text.
        """
        if not old:
            raise ValueError("old must not be empty")
        before = (self._text, self._cursor)
        count = self._text.count(old)
        self._text = self._text.replace(old, new)
        self._cursor = min(self._cursor, len(self._text))
        self._record(before)
        return count
