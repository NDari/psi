import contextlib
import io
import os
import tempfile
import unittest

from tracker.cli import main


class CliTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, "tasks.json")

    def tearDown(self):
        self.dir.cleanup()

    def run_cli(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = main(["--file", self.path, "--today", "2026-03-11", *args])
        return code, out.getvalue(), err.getvalue()

    def test_add_list_done(self):
        self.assertEqual(self.run_cli("add", "Buy", "milk", "-p", "home", "-t", "shop", "--due", "tomorrow")[1],
                         "Added task 1: Buy milk\n")
        self.run_cli("add", "Send invoice", "-p", "work", "--due", "2026-03-09")
        code, out, _ = self.run_cli("list")
        self.assertEqual(code, 0)
        self.assertEqual(out, "  1 [ ] Buy milk (home) due 2026-03-12 #shop\n"
                              "  2 [ ] Send invoice (work) due 2026-03-09!\n")
        self.assertEqual(self.run_cli("done", "2")[1], "Done: 2 Send invoice\n")
        self.assertEqual(self.run_cli("list", "--status", "done")[1],
                         "  2 [x] Send invoice (work) due 2026-03-09\n")

    def test_errors(self):
        code, _, err = self.run_cli("done", "9")
        self.assertEqual(code, 1)
        self.assertEqual(err, "error: no task with id 9\n")
        code, _, err = self.run_cli("add", "x", "--due", "someday")
        self.assertEqual(code, 1)

    def test_edit_and_show(self):
        self.run_cli("add", "Draft", "-t", "a")
        self.assertEqual(self.run_cli("edit", "1", "--title", "Final", "--add-tag", "b")[1],
                         "Updated task 1: title, tags\n")
        out = self.run_cli("show", "1")[1]
        self.assertIn("Title:   Final", out)
        self.assertIn("Tags:    #a #b", out)

    def test_report(self):
        self.run_cli("add", "One", "-p", "home", "--due", "today")
        self.run_cli("add", "Two", "-p", "work")
        self.run_cli("done", "2")
        self.assertEqual(self.run_cli("report")[1],
                         "Tasks: 2 total, 1 open, 1 done\n"
                         "Overdue: 0\n"
                         "Due today: 1\n"
                         "Due this week: 0\n"
                         "By project:\n"
                         "  home: 1 open, 0 done\n"
                         "  work: 0 open, 1 done\n"
                         "Done in last 7 days: 1\n"
                         "Oldest open: #1 One (created 2026-03-11)\n")

    def test_projects_table(self):
        self.run_cli("add", "One", "-p", "home")
        self.run_cli("add", "Two")
        self.assertEqual(self.run_cli("projects")[1],
                         "PROJECT  OPEN  DONE\n"
                         "home        1     0\n"
                         "(none)      1     0\n")


if __name__ == "__main__":
    unittest.main()
