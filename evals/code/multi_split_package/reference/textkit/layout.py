"""Wrapping, alignment and plain-text tables."""

import re

from ._util import _char_width, _split_lines
from .normalize import normalize_newlines, normalize_space

DEFAULT_WIDTH = 72

ELLIPSIS = "..."


def display_width(text):
    """Number of terminal columns text takes.

    Wide East Asian characters count as two, combining marks as zero.

    >>> display_width("abc")
    3
    >>> display_width("日本")
    4
    """
    return sum(_char_width(ch) for ch in text)


def _cut_to_width(text, width):
    """Longest prefix of text whose display width is at most width."""
    total = 0
    for i, ch in enumerate(text):
        w = _char_width(ch)
        if total + w > width:
            return text[:i]
        total += w
    return text


def wrap(text, width=DEFAULT_WIDTH, break_long=True):
    """Wrap one paragraph into lines no wider than width.

    Whitespace is normalized first. A word wider than width is split across
    lines when break_long is true and put on a line of its own otherwise.
    Returns a list of lines (empty for blank text).

    >>> wrap("the quick brown fox", 10)
    ['the quick', 'brown fox']
    """
    if width < 1:
        raise ValueError("width must be at least 1")
    words = normalize_space(text).split(" ")
    if words == [""]:
        return []
    lines = []
    current = ""
    for word in words:
        while break_long and display_width(word) > width:
            if current:
                lines.append(current)
                current = ""
            head = _cut_to_width(word, width)
            lines.append(head)
            word = word[len(head) :]
        if not word:
            continue
        if not current:
            current = word
        elif display_width(current) + 1 + display_width(word) <= width:
            current = current + " " + word
        else:
            lines.append(current)
            current = word
    if current:
        lines.append(current)
    return lines


def fill(text, width=DEFAULT_WIDTH, first_prefix="", prefix=""):
    """Wrap text into paragraphs and join them into one string.

    Paragraphs are separated by blank lines in the input and keep one blank
    line between them in the output. first_prefix starts the first line of
    each paragraph and prefix starts every other line; the widths of the
    prefixes count toward width.

    >>> print(fill("one two three four", 9, "* ", "  "))
    * one two
      three
      four
    """
    paragraphs = re.split(r"\n\s*\n", normalize_newlines(text).strip())
    out = []
    for para in paragraphs:
        if not para.strip():
            continue
        inner = width - max(display_width(first_prefix), display_width(prefix))
        lines = wrap(para, max(inner, 1))
        block = [(first_prefix if i == 0 else prefix) + line for i, line in enumerate(lines)]
        out.append("\n".join(block))
    return "\n\n".join(out)


def indent(text, prefix, skip_blank=True):
    """Put prefix in front of every line of text.

    Blank lines (only whitespace) are left empty when skip_blank is true.
    A trailing newline is kept.
    """
    lines = normalize_newlines(text).split("\n")
    trailing = lines and lines[-1] == ""
    if trailing:
        lines = lines[:-1]
    out = []
    for line in lines:
        if skip_blank and not line.strip():
            out.append("")
        else:
            out.append(prefix + line)
    result = "\n".join(out)
    return result + "\n" if trailing else result


def dedent(text):
    """Remove the common leading whitespace of all non-blank lines.

    Tabs count as one character, like the rest of the whitespace. Blank
    lines become empty.
    """
    lines = _split_lines(text)
    margins = [len(line) - len(line.lstrip()) for line in lines if line.strip()]
    margin = min(margins) if margins else 0
    return "\n".join(line[margin:] if line.strip() else "" for line in lines)


def align(text, width, how="left", fillchar=" "):
    """Pad text to width display columns.

    how is "left", "right" or "center"; centering puts the odd column on
    the right. Text wider than width is returned unchanged.

    >>> align("ab", 5, "center", ".")
    '.ab..'
    """
    if len(fillchar) != 1:
        raise ValueError("fillchar must be one character")
    gap = width - display_width(text)
    if gap <= 0:
        return text
    if how == "left":
        return text + fillchar * gap
    if how == "right":
        return fillchar * gap + text
    if how == "center":
        left = gap // 2
        return fillchar * left + text + fillchar * (gap - left)
    raise ValueError("how must be left, right or center, not %r" % (how,))


def truncate(text, width, ellipsis=ELLIPSIS):
    """Shorten text to at most width display columns.

    When text is cut, the ellipsis is added and counts toward width; the cut
    falls on a word boundary when one exists in the second half of the kept
    text.

    >>> truncate("the quick brown fox", 12)
    'the quick...'
    """
    if display_width(text) <= width:
        return text
    room = width - display_width(ellipsis)
    if room <= 0:
        return _cut_to_width(ellipsis, width)
    head = _cut_to_width(text, room)
    space = head.rfind(" ")
    if space >= len(head) // 2:
        head = head[:space]
    return head.rstrip() + ellipsis


def table(rows, headers=None, aligns=None, sep="  "):
    """Lay out rows as a plain-text table.

    Every cell is converted with str(). Columns are as wide as their widest
    cell. aligns is a string with one letter per column: "l", "r" or "c"
    (default all "l"). With headers, a line of hyphens follows the header
    row. Trailing spaces are stripped from every line. Returns one string
    without a trailing newline.

    >>> print(table([["a", 1], ["bb", 22]], headers=["k", "v"], aligns="lr"))
    k    v
    --  --
    a    1
    bb  22
    """
    body = [[str(c) for c in row] for row in rows]
    head = [str(h) for h in headers] if headers is not None else None
    all_rows = ([head] if head is not None else []) + body
    if not all_rows:
        return ""
    ncols = max(len(r) for r in all_rows)
    all_rows = [r + [""] * (ncols - len(r)) for r in all_rows]
    widths = [max(display_width(r[i]) for r in all_rows) for i in range(ncols)]
    kinds = (aligns or "").ljust(ncols, "l")
    names = {"l": "left", "r": "right", "c": "center"}

    def line(cells):
        padded = [align(c, widths[i], names[kinds[i]]) for i, c in enumerate(cells)]
        return sep.join(padded).rstrip()

    out = []
    if head is not None:
        out.append(line(all_rows[0]))
        out.append(sep.join("-" * w for w in widths).rstrip())
        all_rows = all_rows[1:]
    out.extend(line(r) for r in all_rows)
    return "\n".join(out)
