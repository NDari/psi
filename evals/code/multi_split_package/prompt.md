textkit.py has grown to over 800 lines. Please turn it into a package, a `textkit/` directory, and delete textkit.py. Put each group in its own submodule (the groups match the module docstring):

- `textkit/normalize.py`: normalize_space, normalize_newlines, strip_accents, squeeze, slugify, snake_case, kebab_case, camel_case, title_case, SMALL_WORDS
- `textkit/layout.py`: display_width, wrap, fill, indent, dedent, align, truncate, table, DEFAULT_WIDTH, ELLIPSIS
- `textkit/stats.py`: word_count, char_count, sentence_count, word_frequencies, top_words, syllables, reading_time, readability, summary, STOPWORDS, WORDS_PER_MINUTE
- `textkit/template.py`: Template, TemplateError, render, register_filter, FILTERS, and the built-in filters

The functions and classes must be defined in those submodules, not just imported there. Shared private helpers can go wherever you like. Nothing may change for callers: every name the old module exported (and `textkit.__version__`) must still work as `textkit.X` and `from textkit import X`, and a filter registered with `textkit.register_filter` must work in every render. Behavior stays exactly the same. app.py and report.py use the module; tests are in tests/ (`python3 -m unittest discover tests`).
