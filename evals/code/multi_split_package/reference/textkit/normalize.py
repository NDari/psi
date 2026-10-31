"""Normalizing text: whitespace, accents, slugs and identifier cases."""

import unicodedata

from ._util import _CAMEL_RE, _LIGATURES, _SPACE_RE, _ascii_fold, _words

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
