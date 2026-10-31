"""textkit: small text utilities shared by the notes tools.

The module has four groups of helpers:

* normalizing: normalize_space, normalize_newlines, strip_accents, squeeze,
  slugify, snake_case, kebab_case, camel_case, title_case
* wrapping and layout: display_width, wrap, fill, indent, dedent, align,
  truncate, table
* word statistics: word_count, char_count, sentence_count, word_frequencies,
  top_words, syllables, reading_time, readability, summary
* templating: Template, TemplateError, render, register_filter, FILTERS

Everything is pure Python and has no dependencies outside the standard
library.
"""

import math
import re
import unicodedata
from collections import Counter

__version__ = "1.4.0"

__all__ = [
    # normalizing
    "normalize_space",
    "normalize_newlines",
    "strip_accents",
    "squeeze",
    "slugify",
    "snake_case",
    "kebab_case",
    "camel_case",
    "title_case",
    "SMALL_WORDS",
    # wrapping and layout
    "DEFAULT_WIDTH",
    "ELLIPSIS",
    "display_width",
    "wrap",
    "fill",
    "indent",
    "dedent",
    "align",
    "truncate",
    "table",
    # word statistics
    "STOPWORDS",
    "WORDS_PER_MINUTE",
    "word_count",
    "char_count",
    "sentence_count",
    "word_frequencies",
    "top_words",
    "syllables",
    "reading_time",
    "readability",
    "summary",
    # templating
    "FILTERS",
    "Template",
    "TemplateError",
    "register_filter",
    "render",
]


# ---------------------------------------------------------------------------
# Shared private helpers
# ---------------------------------------------------------------------------

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
    return normalize_newlines(text).split("\n")


# ---------------------------------------------------------------------------
# Normalizing
# ---------------------------------------------------------------------------

SMALL_WORDS = frozenset(
    """a an and as at but by en for if in nor of on or per the to v vs via""".split()
)


def normalize_space(text):
    """Collapse every run of whitespace into one space and strip the ends.

    >>> normalize_space("  a \\t b\\n c ")
    'a b c'
    """
    return _SPACE_RE.sub(" ", text).strip()


def normalize_newlines(text):
    """Turn Windows and old Mac line endings into "\\n"."""
    return text.replace("\r\n", "\n").replace("\r", "\n")


def strip_accents(text):
    """Remove accents and fold ligatures, keeping everything else.

    Unlike _ascii_fold, characters with no ASCII form are kept as they are.

    >>> strip_accents("Crème brûlée")
    'Creme brulee'
    """
    text = text.translate(_LIGATURES)
    decomposed = unicodedata.normalize("NFKD", text)
    out = "".join(c for c in decomposed if not unicodedata.combining(c))
    return unicodedata.normalize("NFC", out)


def squeeze(text, chars=None):
    """Collapse runs of a repeated character into one.

    With chars given, only those characters are squeezed.

    >>> squeeze("aaa--bb", "-")
    'aaa-bb'
    """
    if not text:
        return text
    out = [text[0]]
    for ch in text[1:]:
        if ch == out[-1] and (chars is None or ch in chars):
            continue
        out.append(ch)
    return "".join(out)


def slugify(text, sep="-", max_length=None):
    """Make a lowercase ASCII slug for use in URLs and file names.

    Words are found after folding accents; everything else becomes the
    separator. With max_length, the slug is cut at a word boundary so it is
    at most that long (a single long first word is cut hard).

    >>> slugify("Héllo, Wörld!")
    'hello-world'
    """
    folded = _ascii_fold(text).lower().replace("'", "")
    words = _words(folded)
    slug = sep.join(words)
    if max_length is not None and len(slug) > max_length:
        cut = slug[: max_length + 1]
        if sep in cut:
            cut = cut[: cut.rindex(sep)]
        else:
            cut = cut[:max_length]
        slug = cut.strip(sep)
    return slug


def _identifier_parts(text):
    """Split free text or an identifier into lowercase parts."""
    parts = []
    for word in _words(_ascii_fold(text).replace("'", "")):
        parts.extend(p.lower() for p in _CAMEL_RE.findall(word))
    return parts


def snake_case(text):
    """Convert text or an identifier to snake_case.

    >>> snake_case("parseHTTPResponse")
    'parse_http_response'
    >>> snake_case("Total amount (net)")
    'total_amount_net'
    """
    return "_".join(_identifier_parts(text))


def kebab_case(text):
    """Convert text or an identifier to kebab-case."""
    return "-".join(_identifier_parts(text))


def camel_case(text, upper_first=False):
    """Convert text or an identifier to camelCase (or CamelCase).

    >>> camel_case("total amount net")
    'totalAmountNet'
    """
    parts = _identifier_parts(text)
    if not parts:
        return ""
    head = parts[0].capitalize() if upper_first else parts[0]
    return head + "".join(p.capitalize() for p in parts[1:])


def title_case(text):
    """Title-case a heading, keeping small words lowercase.

    The first and last words are always capitalized. Words that already
    contain a capital letter after their first character (iPhone, NASA)
    are left alone.

    >>> title_case("the lord of the rings")
    'The Lord of the Rings'
    """
    pieces = normalize_space(text).split(" ")
    if pieces == [""]:
        return ""
    out = []
    last = len(pieces) - 1
    for i, piece in enumerate(pieces):
        if any(c.isupper() for c in piece[1:]):
            out.append(piece)
        elif 0 < i < last and piece.lower() in SMALL_WORDS:
            out.append(piece.lower())
        else:
            out.append(_capitalize_word(piece))
    return " ".join(out)


def _capitalize_word(piece):
    """Capitalize the first letter of piece, skipping leading punctuation.

    Hyphenated words get each part capitalized ("well-known" becomes
    "Well-Known").
    """
    if "-" in piece:
        return "-".join(_capitalize_word(p) for p in piece.split("-"))
    for i, ch in enumerate(piece):
        if ch.isalpha():
            return piece[:i] + ch.upper() + piece[i + 1 :].lower()
    return piece


# ---------------------------------------------------------------------------
# Wrapping and layout
# ---------------------------------------------------------------------------

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


# ---------------------------------------------------------------------------
# Word statistics
# ---------------------------------------------------------------------------

STOPWORDS = frozenset(
    """
    a about above after again against all am an and any are as at be because
    been before being below between both but by can could did do does doing
    down during each few for from further had has have having he her here
    hers him his how i if in into is it its itself just me more most my no
    nor not now of off on once only or other our ours out over own same she
    should so some such than that the their theirs them then there these
    they this those through to too under until up very was we were what when
    where which while who whom why will with would you your yours
    """.split()
)

WORDS_PER_MINUTE = 200

_VOWEL_GROUP_RE = re.compile(r"[aeiouy]+")


def word_count(text):
    """Number of words in text.

    >>> word_count("Don't panic, it's fine.")
    4
    """
    return len(_words(text))


def char_count(text, spaces=True):
    """Number of characters, optionally leaving out whitespace."""
    if spaces:
        return len(text)
    return sum(1 for c in text if not c.isspace())


def sentence_count(text):
    """Number of sentences, counting runs of . ! ? that end a sentence.

    Text that has words but no closing punctuation is one sentence.
    """
    if not _words(text):
        return 0
    found = len(_SENTENCE_END_RE.findall(text.strip()))
    stripped = text.strip()
    if not stripped or stripped[-1] not in ".!?":
        found += 1
    return found


def word_frequencies(text, fold=True):
    """Count each word. With fold, case and accents are ignored.

    Returns a Counter.
    """
    words = _words(text)
    if fold:
        words = [_ascii_fold(w).lower() or w.lower() for w in words]
    return Counter(words)


def top_words(text, n=5, skip_stopwords=True):
    """The n most common words as (word, count) pairs.

    Words are folded as in word_frequencies. Ties are broken alphabetically.
    Stop words and one-letter words are skipped when skip_stopwords is true.
    """
    freq = word_frequencies(text)
    items = [
        (w, c)
        for w, c in freq.items()
        if not skip_stopwords or (w not in STOPWORDS and len(w) > 1)
    ]
    items.sort(key=lambda wc: (-wc[1], wc[0]))
    return items[:n]


def syllables(word):
    """Rough number of syllables in an English word (at least 1).

    Counts vowel groups, drops a silent final "e" and counts a final "le"
    after a consonant.
    """
    w = _ascii_fold(word).lower()
    w = "".join(c for c in w if c.isalpha())
    if not w:
        return 0
    count = len(_VOWEL_GROUP_RE.findall(w))
    if w.endswith("e") and not w.endswith(("le", "ee", "ye")) and count > 1:
        count -= 1
    if w.endswith("le") and len(w) > 2 and w[-3] not in "aeiouy":
        if count == 0 or not _VOWEL_GROUP_RE.search(w[-2:]):
            count += 1
    return max(count, 1)


def reading_time(text, wpm=WORDS_PER_MINUTE):
    """Whole minutes needed to read text, rounded up; 0 for no words."""
    words = word_count(text)
    if words == 0:
        return 0
    return math.ceil(words / wpm)


def readability(text):
    """Flesch reading ease of text, rounded to one decimal.

    Higher is easier. Returns 0.0 when text has no words.
    """
    words = _words(text)
    if not words:
        return 0.0
    sentences = max(sentence_count(text), 1)
    syl = sum(syllables(w) for w in words)
    score = 206.835 - 1.015 * (len(words) / sentences) - 84.6 * (syl / len(words))
    return round(score, 1)


def summary(text):
    """A dict of the main statistics of text.

    Keys: words, characters, sentences, reading_minutes, readability, top.
    """
    return {
        "words": word_count(text),
        "characters": char_count(text),
        "sentences": sentence_count(text),
        "reading_minutes": reading_time(text),
        "readability": readability(text),
        "top": top_words(text, 3),
    }


# ---------------------------------------------------------------------------
# Templating
# ---------------------------------------------------------------------------
#
# Templates use {{ expression }} with an optional chain of filters:
#
#     Hello {{ user.name | title }}, you have {{ count | default:0 }} notes.
#
# A filter may take arguments after a colon, separated by commas. Text
# between {{! and }} is a comment. A backslash before {{ prints {{ as is.

FILTERS = {}


class TemplateError(Exception):
    """Raised for malformed templates, unknown filters and, in strict mode,
    missing variables."""


def register_filter(name):
    """Decorator that adds a function to FILTERS under name.

    The function gets the current value first, then the filter arguments as
    strings.

        @register_filter("shout")
        def shout(value):
            return str(value).upper() + "!"
    """

    def decorator(func):
        FILTERS[name] = func
        return func

    return decorator


_TAG_RE = re.compile(r"(\\?)\{\{(.*?)\}\}", re.S)

_MISSING = object()


def _lookup(context, dotted):
    """Resolve a dotted name such as "user.name" against context."""
    value = context
    for part in dotted.split("."):
        if isinstance(value, dict):
            value = value.get(part, _MISSING)
        elif isinstance(value, (list, tuple)) and part.isdigit():
            idx = int(part)
            value = value[idx] if idx < len(value) else _MISSING
        else:
            value = getattr(value, part, _MISSING)
        if value is _MISSING:
            return _MISSING
    return value


def _parse_filter(spec):
    """Split "name:arg1,arg2" into ("name", ["arg1", "arg2"])."""
    name, _, rest = spec.partition(":")
    name = name.strip()
    args = [a.strip() for a in rest.split(",")] if rest.strip() else []
    return name, args


class Template:
    """A parsed template that can be rendered many times."""

    def __init__(self, source, strict=False):
        self.source = source
        self.strict = strict
        self._parts = self._parse(source)

    @staticmethod
    def _parse(source):
        """Split source into literal strings and (expression, filters) tags."""
        parts = []
        pos = 0
        for m in _TAG_RE.finditer(source):
            parts.append(source[pos : m.start()])
            pos = m.end()
            if m.group(1):
                parts.append("{{" + m.group(2) + "}}")
                continue
            body = m.group(2).strip()
            if body.startswith("!"):
                continue
            if not body:
                raise TemplateError("empty tag at offset %d" % m.start())
            pieces = [p.strip() for p in body.split("|")]
            expr = pieces[0]
            if not re.fullmatch(r"[A-Za-z_][\w.]*", expr):
                raise TemplateError("bad expression %r" % expr)
            filters = [_parse_filter(p) for p in pieces[1:]]
            for fname, _ in filters:
                if not fname:
                    raise TemplateError("empty filter in %r" % body)
            parts.append((expr, filters))
        tail = source[pos:]
        if "{{" in tail.replace("\\{{", ""):
            raise TemplateError("unclosed tag")
        parts.append(tail)
        return parts

    def render(self, context=None, **extra):
        """Render with context (a dict) plus keyword arguments."""
        ctx = dict(context or {})
        ctx.update(extra)
        out = []
        for part in self._parts:
            if isinstance(part, str):
                out.append(part)
                continue
            expr, filters = part
            value = _lookup(ctx, expr)
            if value is _MISSING:
                if self.strict and not any(f == "default" for f, _ in filters):
                    raise TemplateError("missing variable %r" % expr)
                value = None
            for fname, args in filters:
                func = FILTERS.get(fname)
                if func is None:
                    raise TemplateError("unknown filter %r" % fname)
                value = func(value, *args)
            out.append("" if value is None else str(value))
        return "".join(out)


def render(source, context=None, strict=False, **extra):
    """Parse and render a template in one call.

    >>> render("Hi {{ name | upper }}!", {"name": "ada"})
    'Hi ADA!'
    """
    return Template(source, strict=strict).render(context, **extra)


def _text(value):
    return "" if value is None else str(value)


@register_filter("upper")
def _f_upper(value):
    return _text(value).upper()


@register_filter("lower")
def _f_lower(value):
    return _text(value).lower()


@register_filter("title")
def _f_title(value):
    return title_case(_text(value))


@register_filter("slug")
def _f_slug(value, sep="-"):
    return slugify(_text(value), sep=sep)


@register_filter("trim")
def _f_trim(value):
    return normalize_space(_text(value))


@register_filter("truncate")
def _f_truncate(value, width="30"):
    return truncate(_text(value), int(width))


@register_filter("pad")
def _f_pad(value, width, how="left"):
    return align(_text(value), int(width), how)


@register_filter("default")
def _f_default(value, fallback=""):
    return fallback if value is None or value == "" else value


@register_filter("count")
def _f_count(value):
    return word_count(_text(value))


@register_filter("join")
def _f_join(value, sep=", "):
    if value is None:
        return ""
    return sep.join(_text(v) for v in value)


@register_filter("plural")
def _f_plural(value, singular="", plural="s"):
    try:
        n = int(value)
    except (TypeError, ValueError):
        raise TemplateError("plural needs a number, got %r" % (value,))
    return singular if n == 1 else plural
