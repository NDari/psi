"""Private helpers shared by the textkit submodules."""

import re
import unicodedata

# A word is a run of letters or digits, optionally followed by an apostrophe
# and more letters ("don't", "it's"). Accented letters count as letters.
_WORD_RE = re.compile(r"[^\W_]+(?:'[^\W_]+)?")

# Boundaries inside identifiers such as "parseHTTPResponse2Fast".
_CAMEL_RE = re.compile(r"[A-Z]+(?=[A-Z][a-z])|[A-Z]?[a-z]+|[A-Z]+|[0-9]+")

_SENTENCE_END_RE = re.compile(r"[.!?]+(?=\s|$)")

_SPACE_RE = re.compile(r"\s+")


def _words(text):
    """Return the words of text in order, as written."""
    return _WORD_RE.findall(text)


def _ascii_fold(text):
    """Fold text to plain ASCII where a close equivalent exists.

    Letters with accents lose them, a few common ligatures are spelled out,
    and characters with no ASCII form are dropped.
    """
    text = text.translate(_LIGATURES)
    decomposed = unicodedata.normalize("NFKD", text)
    kept = [c for c in decomposed if not unicodedata.combining(c)]
    return "".join(kept).encode("ascii", "ignore").decode("ascii")


_LIGATURES = str.maketrans(
    {
        "ß": "ss",
        "æ": "ae",
        "Æ": "AE",
        "œ": "oe",
        "Œ": "OE",
        "ø": "o",
        "Ø": "O",
        "ł": "l",
        "Ł": "L",
        "ð": "d",
        "þ": "th",
    }
)


def _char_width(ch):
    """Columns a single character takes in a terminal."""
    if unicodedata.combining(ch):
        return 0
    if unicodedata.category(ch) in ("Cc", "Cf"):
        return 0
    if unicodedata.east_asian_width(ch) in ("W", "F"):
        return 2
    return 1


def _split_lines(text):
    """Split text into lines, accepting any newline convention."""
    return text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
