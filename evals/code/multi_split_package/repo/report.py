"""Build a plain-text report over a directory of notes."""

import os

import textkit

REPORT_TEMPLATE = """{{ title | upper }}
{{ rule }}

{{ count }} note{{ count | plural }}, {{ words }} words, about {{ minutes | minutes }} of reading.

{{ table }}

Most used words: {{ top | join }}
"""


@textkit.register_filter("minutes")
def _minutes(value):
    n = int(value)
    if n == 0:
        return "no time"
    return "%d minute%s" % (n, "" if n == 1 else "s")


def load_notes(directory):
    """Read every .txt file in directory, sorted by file name.

    Returns (name, text) pairs where name is the file name without .txt.
    """
    notes = []
    for entry in sorted(os.listdir(directory)):
        if entry.endswith(".txt"):
            with open(os.path.join(directory, entry), encoding="utf-8") as f:
                notes.append((entry[:-4], f.read()))
    return notes


def build_report(notes, title="Notes report", width=60):
    """Return the report text for a list of (name, text) pairs."""
    rows = []
    everything = []
    for name, text in notes:
        first = textkit.normalize_space(text.split("\n", 1)[0])
        rows.append(
            [
                textkit.slugify(name),
                textkit.truncate(textkit.title_case(first), 28),
                textkit.word_count(text),
                textkit.readability(text),
            ]
        )
        everything.append(text)
    joined = "\n\n".join(everything)
    grid = textkit.table(rows, headers=["note", "first line", "words", "ease"], aligns="llrr")
    top = ["%s (%d)" % wc for wc in textkit.top_words(joined, 5)]
    return textkit.render(
        REPORT_TEMPLATE,
        title=title,
        rule="=" * min(width, len(title)),
        count=len(notes),
        words=textkit.word_count(joined),
        minutes=textkit.reading_time(joined),
        table=grid,
        top=top,
    )
