"""The text model behind the note editor: a string plus a cursor.

The cursor is an index into the text, 0 <= cursor <= len(text). Edits
happen at the cursor, like typing.
"""


class TextBuffer:
    def __init__(self, text=""):
        self._text = text
        self._cursor = len(text)

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
        c = self._cursor
        self._text = self._text[:c] + s + self._text[c:]
        self._cursor = c + len(s)

    def delete(self, n=1):
        """Delete up to n characters before the cursor (like backspace).

        Returns the number of characters deleted.
        """
        if n < 0:
            raise ValueError("n must not be negative")
        c = self._cursor
        n = min(n, c)
        self._text = self._text[:c - n] + self._text[c:]
        self._cursor = c - n
        return n

    def replace_all(self, old, new):
        """Replace every occurrence of old with new. Returns the count.

        The cursor stays at the same index, clamped to the new text.
        """
        if not old:
            raise ValueError("old must not be empty")
        count = self._text.count(old)
        self._text = self._text.replace(old, new)
        self._cursor = min(self._cursor, len(self._text))
        return count
