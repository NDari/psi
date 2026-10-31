import os
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))

# Each expression is evaluated with "tk" bound to the textkit package; its
# repr must equal the line recorded from the original single-file module.
CALLS = [
    'tk.normalize_space("  a \\t b\\n c ")',
    'tk.normalize_newlines("a\\r\\nb\\rc")',
    'tk.strip_accents("Crème brûlée, Straße, Ærø")',
    'tk.squeeze("aaa--bb  c", "- ")',
    'tk.squeeze("aaabbb")',
    'tk.slugify("Héllo, Wörld! It\'s 2024")',
    'tk.slugify("one two three four", sep="_", max_length=9)',
    'tk.slugify("Supercalifragilistic", max_length=5)',
    'tk.snake_case("parseHTTPResponse2Fast")',
    'tk.kebab_case("Total amount (net)")',
    'tk.camel_case("user id list", upper_first=True)',
    'tk.camel_case("")',
    'tk.title_case("a tale of two cities via the well-known iPhone app")',
    'tk.title_case("   ")',
    'tk.display_width("日本語 ok")',
    'tk.wrap("the quick brown fox jumps over the lazy dog", 12)',
    'tk.wrap("abcdefghijkl mn", 5)',
    'tk.wrap("abcdefghijkl mn", 5, break_long=False)',
    'tk.fill("one two three four\\n\\nfive six seven", 9, "* ", "  ")',
    'tk.indent("a\\n\\nb\\n", "> ")',
    'tk.indent("a\\n  \\nb", "> ", skip_blank=False)',
    'tk.dedent("    x\\n      y\\n\\n    z")',
    'tk.align("ab", 6, "right", "*")',
    'tk.align("abc", 6, "center")',
    'tk.truncate("the quick brown fox jumps", 14)',
    'tk.truncate("abcdefghijklmnop", 8)',
    'tk.truncate("abc", 2)',
    'tk.table([["x", 1.5, "c"], ["yy", 10]], headers=["a", "b"], aligns="rc")',
    'tk.table([])',
    'tk.word_count("Don\'t panic, it\'s fine. 42 times")',
    'tk.char_count("a b\\tc", spaces=False)',
    'tk.sentence_count("One. Two!! Three? four")',
    'tk.sentence_count("...")',
    'sorted(tk.word_frequencies("Café cafe CAFÉ tea").items())',
    'sorted(tk.word_frequencies("Café cafe tea", fold=False).items())',
    'tk.top_words("the cat and the hat and a bat; cat hat cat", 3)',
    'tk.top_words("the the a", 2, skip_stopwords=False)',
    '[tk.syllables(w) for w in ["table", "make", "readability", "free", "rhythm", "a"]]',
    'tk.reading_time("word " * 401)',
    'tk.reading_time("word " * 30, wpm=10)',
    'tk.readability("The cat sat on the mat. It was happy.")',
    'tk.summary("Short text here. Another sentence follows!")',
    'tk.render("Hi {{ user.name | title }}, {{ n }} note{{ n | plural }}", {"user": {"name": "ada lovelace"}, "n": 2})',
    'tk.render("{{ items.1 }} {{ missing | default:none }} \\\\{{ raw }} {{! comment }}end", items=["a", "b"])',
    'tk.render("{{ t | truncate:10 | upper }} {{ t | slug:_ }} {{ t | count }}", t="Hello there wide world")',
    'tk.render("[{{ x | pad:5,right }}] {{ xs | join:/ }}", x="ab", xs=[1, 2, 3])',
    'tk.Template("{{ a }}{{ b }}").render({"a": 1}, b=2)',
    'tk.DEFAULT_WIDTH, tk.ELLIPSIS, tk.WORDS_PER_MINUTE, tk.__version__',
    'len(tk.STOPWORDS), sorted(tk.SMALL_WORDS)[:5]',
    'sorted(n for n in ["upper", "lower", "title", "slug", "trim", "truncate", "pad", "default", "count", "join", "plural"] if n in tk.FILTERS)',
]

ERRORS = [
    ('tk.render("{{ x | nope }}", x=1)', "TemplateError"),
    ('tk.render("{{ x }}", strict=True)', "TemplateError"),
    ('tk.render("{{ x ")', "TemplateError"),
    ('tk.align("a", 3, "middle")', "ValueError"),
    ('tk.wrap("a", 0)', "ValueError"),
]


class BehaviorTest(unittest.TestCase):
    def test_behavior_unchanged(self):
        import textkit as tk

        with open(os.path.join(HERE, "behavior_expected.txt"), encoding="utf-8") as f:
            want = f.read().split("\n")
        for i, expr in enumerate(CALLS):
            self.assertEqual(repr(eval(expr, {"tk": tk})), want[i], expr)
        for expr, exc in ERRORS:
            with self.assertRaises(Exception, msg=expr) as cm:
                eval(expr, {"tk": tk})
            self.assertEqual(type(cm.exception).__name__, exc, expr)
        self.assertTrue(issubclass(tk.TemplateError, Exception))


if __name__ == "__main__":
    import textkit as tk

    for expr in CALLS:
        print(repr(eval(expr, {"tk": tk})))
