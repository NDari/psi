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

from .layout import (
    DEFAULT_WIDTH,
    ELLIPSIS,
    align,
    dedent,
    display_width,
    fill,
    indent,
    table,
    truncate,
    wrap,
)
from .normalize import (
    SMALL_WORDS,
    camel_case,
    kebab_case,
    normalize_newlines,
    normalize_space,
    slugify,
    snake_case,
    squeeze,
    strip_accents,
    title_case,
)
from .stats import (
    STOPWORDS,
    WORDS_PER_MINUTE,
    char_count,
    readability,
    reading_time,
    sentence_count,
    summary,
    syllables,
    top_words,
    word_count,
    word_frequencies,
)
from .template import FILTERS, Template, TemplateError, register_filter, render

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
