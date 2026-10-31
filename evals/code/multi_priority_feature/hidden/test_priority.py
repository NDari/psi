import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from datetime import date

PROJECT = os.getcwd()
HIDDEN = os.path.dirname(os.path.abspath(__file__))
TODAY = "2026-03-11"


def tasks_sample():
    from tracker.model import Task

    return [
        Task(id=1, title="Buy milk", project="home", created=date(2026, 3, 1)),
        Task(id=2, title="Fix bike", project="home", priority="low", created=date(2026, 2, 1)),
        Task(id=3, title="Send invoice", project="work", priority="high", created=date(2026, 3, 2)),
        Task(id=4, title="Archive mail", priority="high", status="done", done_on=date(2026, 3, 10)),
        Task(id=5, title="Plan trip", priority="normal"),
        Task(id=6, title="Call bank", project="work", priority="high"),
        Task(id=7, title="Water plants", priority="low", status="done", done_on=date(2026, 3, 8)),
    ]


class Base(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def fixture(self, name):
        dst = os.path.join(self.dir, name)
        shutil.copy(os.path.join(HIDDEN, name), dst)
        return dst

    def cli(self, path, *args):
        p = subprocess.run(
            [sys.executable, "-m", "tracker", "--file", path, "--today", TODAY, *args],
            cwd=PROJECT, capture_output=True, text=True, timeout=30,
        )
        return p.returncode, p.stdout, p.stderr


class ModelTest(Base):
    def test_model_default_and_values(self):
        from tracker.model import Task

        self.assertEqual(Task(id=1, title="x").priority, "normal")
        for p in ("low", "normal", "high"):
            self.assertEqual(Task(id=1, title="x", priority=p).priority, p)

    def test_model_rejects_unknown_priority(self):
        from tracker.model import Task

        for bad in ("urgent", "critical"):
            with self.assertRaises(ValueError):
                Task(id=1, title="x", priority=bad)


class StorageTest(Base):
    def test_storage_saves_priority(self):
        from tracker import storage

        path = os.path.join(self.dir, "t.json")
        tasks = tasks_sample()
        storage.save(path, tasks)
        with open(path) as f:
            raw = json.load(f)
        entries = raw["tasks"] if isinstance(raw, dict) else raw
        self.assertEqual({e["id"]: e["priority"] for e in entries},
                         {1: "normal", 2: "low", 3: "high", 4: "high", 5: "normal", 6: "high", 7: "low"})
        loaded = storage.load(path)
        self.assertEqual([t.priority for t in loaded], [t.priority for t in tasks])

    def test_storage_old_file_loads_as_normal(self):
        from tracker import storage

        loaded = storage.load(self.fixture("old.json"))
        self.assertEqual([t.id for t in loaded], [1, 2, 3, 4, 5])
        self.assertEqual({t.priority for t in loaded}, {"normal"})
        self.assertEqual([t.status for t in loaded], ["open", "open", "open", "done", "open"])
        self.assertEqual(loaded[1].notes, "back tyre")


class FilterTest(Base):
    def test_filter_by_priority(self):
        from tracker.filters import filter_tasks

        self.assertEqual([t.id for t in filter_tasks(tasks_sample(), status="all", priority="high")], [3, 4, 6])
        self.assertEqual([t.id for t in filter_tasks(tasks_sample(), status="open", priority="high")], [3, 6])
        self.assertEqual([t.id for t in filter_tasks(tasks_sample(), status="all", priority="low")], [2, 7])
        self.assertEqual([t.id for t in filter_tasks(tasks_sample(), status="open", project="home")], [1, 2])

    def test_sort_by_priority(self):
        from tracker.filters import sort_tasks

        shuffled = [tasks_sample()[i] for i in (6, 0, 5, 3, 1, 4, 2)]
        self.assertEqual([t.id for t in sort_tasks(shuffled, "priority")], [3, 4, 6, 1, 5, 2, 7])
        self.assertEqual([t.id for t in sort_tasks(shuffled, "id")], [1, 2, 3, 4, 5, 6, 7])


class CliTest(Base):
    def test_cli_add_priority(self):
        from tracker import storage

        path = os.path.join(self.dir, "t.json")
        self.assertEqual(self.cli(path, "add", "Pay rent", "--priority", "high")[0], 0)
        self.assertEqual(self.cli(path, "add", "Read book")[0], 0)
        self.assertEqual(self.cli(path, "add", "Nap", "--priority", "low", "-p", "home")[0], 0)
        self.assertNotEqual(self.cli(path, "add", "Bad", "--priority", "urgent")[0], 0)
        loaded = storage.load(path)
        self.assertEqual([(t.title, t.priority) for t in loaded],
                         [("Pay rent", "high"), ("Read book", "normal"), ("Nap", "low")])
        self.assertEqual(loaded[2].project, "home")

    def test_cli_list_sort_and_filter(self):
        path = self.fixture("mixed.json")
        code, out, err = self.cli(path, "list", "--status", "all", "--sort", "priority")
        self.assertEqual(code, 0, err)
        self.assertEqual(out,
                         "  3 [ ] Send invoice (work) due 2026-03-09! #money\n"
                         "  4 [x] Archive mail\n"
                         "  6 [ ] Call bank (work) due 2026-03-11\n"
                         "  1 [ ] Buy milk (home) due 2026-03-12 #shop\n"
                         "  5 [ ] Plan trip due 2026-03-20 #travel\n"
                         "  2 [ ] Fix bike (home)\n"
                         "  7 [x] Water plants (home)\n")
        code, out, err = self.cli(path, "list", "--priority", "high")
        self.assertEqual(code, 0, err)
        self.assertEqual(out,
                         "  3 [ ] Send invoice (work) due 2026-03-09! #money\n"
                         "  6 [ ] Call bank (work) due 2026-03-11\n")
        code, out, err = self.cli(path, "list", "--priority", "low", "--status", "all", "--sort", "title")
        self.assertEqual(out, "  2 [ ] Fix bike (home)\n  7 [x] Water plants (home)\n")

    def test_cli_report(self):
        path = self.fixture("mixed.json")
        code, out, err = self.cli(path, "report")
        self.assertEqual(code, 0, err)
        self.assertEqual(out,
                         "Tasks: 7 total, 5 open, 2 done\n"
                         "Overdue: 1\n"
                         "Due today: 1\n"
                         "Priority: high 2, normal 2, low 1\n"
                         "Due this week: 1\n"
                         "By project:\n"
                         "  home: 2 open, 1 done\n"
                         "  work: 2 open, 0 done\n"
                         "  (none): 1 open, 1 done\n"
                         "Done in last 7 days: 2\n"
                         "Oldest open: #2 Fix bike (created 2026-02-01)\n")

    def test_cli_old_commands_still_work(self):
        path = self.fixture("old.json")
        code, out, err = self.cli(path, "list", "--status", "all", "--sort", "due")
        self.assertEqual(code, 0, err)
        self.assertEqual(out,
                         "  3 [ ] Send invoice (work) due 2026-03-09! #money\n"
                         "  1 [ ] Buy milk (home) due 2026-03-12 #shop\n"
                         "  5 [ ] Plan trip due 2026-03-20 #travel\n"
                         "  2 [ ] Fix bike (home)\n"
                         "  4 [x] Archive mail\n")
        self.assertEqual(self.cli(path, "add", "Book dentist", "-p", "home", "--due", "fri")[1],
                         "Added task 6: Book dentist\n")
        self.assertEqual(self.cli(path, "done", "1", "3")[1], "Done: 1 Buy milk\nDone: 3 Send invoice\n")
        self.assertEqual(self.cli(path, "list", "-p", "home")[1],
                         "  2 [ ] Fix bike (home)\n"
                         "  6 [ ] Book dentist (home) due 2026-03-13\n")
        self.assertEqual(self.cli(path, "projects")[1],
                         "PROJECT  OPEN  DONE\n"
                         "home        2     1\n"
                         "work        0     1\n"
                         "(none)      1     1\n")
