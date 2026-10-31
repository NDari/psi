import importlib
import os
import unittest

GROUPS = {
    "normalize": [
        "normalize_space",
        "normalize_newlines",
        "strip_accents",
        "squeeze",
        "slugify",
        "snake_case",
        "kebab_case",
        "camel_case",
        "title_case",
    ],
    "layout": ["display_width", "wrap", "fill", "indent", "dedent", "align", "truncate", "table"],
    "stats": [
        "word_count",
        "char_count",
        "sentence_count",
        "word_frequencies",
        "top_words",
        "syllables",
        "reading_time",
        "readability",
        "summary",
    ],
    "template": ["Template", "TemplateError", "render", "register_filter"],
}

CONSTANTS = {
    "normalize": ["SMALL_WORDS"],
    "layout": ["DEFAULT_WIDTH", "ELLIPSIS"],
    "stats": ["STOPWORDS", "WORDS_PER_MINUTE"],
    "template": ["FILTERS"],
}


class StructureTest(unittest.TestCase):
    def test_package(self):
        import textkit

        self.assertTrue(hasattr(textkit, "__path__"), "textkit is a plain module, not a package")
        self.assertEqual(
            os.path.realpath(textkit.__file__),
            os.path.realpath(os.path.join(os.getcwd(), "textkit", "__init__.py")),
        )

    def _group(self, name):
        import textkit

        mod = importlib.import_module("textkit." + name)
        for fn in GROUPS[name]:
            obj = getattr(mod, fn)
            self.assertEqual(obj.__module__, "textkit." + name, fn)
            self.assertIs(getattr(textkit, fn), obj, "textkit.%s is not textkit.%s.%s" % (fn, name, fn))
        for const in CONSTANTS[name]:
            self.assertIs(getattr(textkit, const), getattr(mod, const), const)

    def test_normalize_module(self):
        self._group("normalize")

    def test_layout_module(self):
        self._group("layout")

    def test_stats_module(self):
        self._group("stats")

    def test_template_module(self):
        self._group("template")

    def test_one_filter_registry(self):
        import textkit
        template = importlib.import_module("textkit.template")

        @textkit.register_filter("hidden_rev")
        def rev(value):
            return str(value)[::-1]

        self.assertEqual(template.render("{{ x | hidden_rev }}", x="abc"), "cba")
        self.assertEqual(template.Template("{{ x | hidden_rev | upper }}").render(x="ab"), "BA")

        @template.register_filter("hidden_twice")
        def twice(value):
            return str(value) * 2

        self.assertEqual(textkit.render("{{ x | hidden_twice }}", x="ab"), "abab")
        self.assertIn("hidden_twice", textkit.FILTERS)
        self.assertIn("slug", template.FILTERS)
