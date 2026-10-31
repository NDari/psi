"""Word statistics: counts, frequencies and readability."""

import math
import re
from collections import Counter

from ._util import _SENTENCE_END_RE, _ascii_fold, _words

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
