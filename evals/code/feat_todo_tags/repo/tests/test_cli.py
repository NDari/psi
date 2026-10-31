import contextlib
import io
import json
import os
import tempfile
import unittest
from unittest import mock

from todo.cli import main


class CliTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, "todo.json")
        env = mock.patch.dict(os.environ, {"TODO_FILE": self.path, "TODO_TODAY": "2026-10-28"})
        env.start()
        self.addCleanup(env.stop)
        self.addCleanup(self.dir.cleanup)

    def run_cli(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = main(list(argv))
        return code, out.getvalue(), err.getvalue()

    def test_add_and_list(self):
        self.assertEqual(self.run_cli("add", "Pay rent", "-p", "1", "--due", "fri")[1], "added 1\n")
        self.run_cli("add", "Water plants")
        code, out, _ = self.run_cli("list")
        self.assertEqual(out, "  1 [ ] (p1) Pay rent  due 2026-10-30\n  2 [ ] (p2) Water plants\n")

    def test_done_hides_task(self):
        self.run_cli("add", "a")
        self.run_cli("add", "b")
        self.run_cli("done", "1")
        self.assertEqual(self.run_cli("list")[1], "  2 [ ] (p2) b\n")
        self.assertEqual(self.run_cli("list", "--all")[1], "  1 [x] (p2) a\n  2 [ ] (p2) b\n")

    def test_overdue_and_sort(self):
        self.run_cli("add", "late", "--due", "2026-10-01")
        self.run_cli("add", "urgent", "-p", "1")
        out = self.run_cli("list", "--sort", "priority")[1]
        self.assertEqual(out, "  2 [ ] (p1) urgent\n  1 [ ] (p2) late  due 2026-10-01 !\n")

    def test_errors(self):
        code, out, err = self.run_cli("rm", "9")
        self.assertEqual((code, out, err), (1, "", "todo: no task 9\n"))

    def test_file_format(self):
        self.run_cli("add", "x")
        with open(self.path) as f:
            data = json.load(f)
        self.assertEqual(data["version"], 1)
        self.assertEqual(data["tasks"][0]["title"], "x")


if __name__ == "__main__":
    unittest.main()
