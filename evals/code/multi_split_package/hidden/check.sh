. /evallib/check.sh

unittests /hidden/test_structure.py /hidden/test_behavior.py

# Each submodule imports on its own in a fresh interpreter.
check submodules_import_alone python3 -c '
import importlib, sys
for name in ("textkit.stats", "textkit.template", "textkit.layout", "textkit.normalize"):
    for m in [m for m in sys.modules if m == "textkit" or m.startswith("textkit.")]:
        del sys.modules[m]
    importlib.import_module(name)
from textkit.template import render
assert render("{{ t | title | truncate:12 }}", t="the art of war and peace") == "The Art..."
from textkit.stats import top_words
assert top_words("cat dog cat", 1) == [("cat", 2)]
'

check_output app_report /hidden/report_expected.txt python3 app.py report /hidden/notes

check visible_tests python3 -m unittest discover tests
