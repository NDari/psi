"""Run hidden unittest files against the graded project, one result line
per test: "PASS <id>" or "FAIL <id>: <reason>" (eval/code/README.md).

Usage: python3 /evallib/unittest_check.py /hidden/test_a.py [/hidden/test_b.py ...]

The working directory is the graded project; it is put first on sys.path,
so the tests import the project's modules as the model left them.
"""
import importlib.util
import os
import sys
import unittest

sys.path.insert(0, os.getcwd())


class Lines(unittest.TestResult):
    def addSuccess(self, test):
        print(f"PASS {test.id()}", flush=True)

    def _fail(self, test, err):
        msg = str(err[1]).strip().splitlines()
        print(f"FAIL {test.id()}: {err[0].__name__}: {msg[-1] if msg else ''}"[:300], flush=True)

    addFailure = _fail
    addError = _fail

    def addSkip(self, test, reason):
        print(f"FAIL {test.id()}: skipped: {reason}", flush=True)


for path in sys.argv[1:]:
    name = os.path.splitext(os.path.basename(path))[0]
    try:
        spec = importlib.util.spec_from_file_location(name, path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
    except BaseException as e:  # the project failed to import: every test fails
        print(f"FAIL {name}: import: {type(e).__name__}: {e}"[:300], flush=True)
        continue
    suite = unittest.defaultTestLoader.loadTestsFromModule(mod)
    if suite.countTestCases() == 0:
        print(f"FAIL {name}: no tests", flush=True)
    suite.run(Lines())
