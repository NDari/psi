import json
import os
import subprocess
import sys
import tempfile
import unittest

from notes.model import Note


class DirStoreTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = os.path.join(self.tmp.name, "store")

    def make(self, path=None):
        from notes.store.dirstore import DirStore
        return DirStore(path or self.dir)

    def test_roundtrip_and_replace(self):
        s = self.make()
        n = Note("Groceries", "milk\neggs", ("home", "weekly"), 1700000000)
        s.put("groceries", n)
        self.assertEqual(s.get("groceries"), n)
        s.put("groceries", Note("Groceries", "bread"))
        self.assertEqual(s.get("groceries"), Note("Groceries", "bread"))
        self.assertEqual(len(s), 1)
        self.assertIn("groceries", s)
        self.assertNotIn("other", s)

    def test_missing_keys(self):
        s = self.make()
        with self.assertRaises(KeyError):
            s.get("absent")
        with self.assertRaises(KeyError):
            s.delete("absent")

    def test_invalid_keys(self):
        s = self.make()
        for bad in ["", "Caps", "_x", "../etc", "a.b", "x" * 65]:
            with self.assertRaises(ValueError, msg=bad):
                s.put(bad, Note("t"))
            with self.assertRaises(ValueError, msg=bad):
                s.get(bad)
            with self.assertRaises(ValueError, msg=bad):
                s.delete(bad)

    def test_keys_prefix_delete(self):
        s = self.make()
        for k in ["work-2", "home", "work-10", "work-1"]:
            s.put(k, Note(k, "about " + k, ("t",) if k.startswith("work") else ()))
        self.assertEqual(s.keys(), ["home", "work-1", "work-10", "work-2"])
        self.assertEqual(s.keys("work-1"), ["work-1", "work-10"])
        self.assertEqual(s.keys("zzz"), [])
        self.assertEqual(s.search("ABOUT WORK-1"), ["work-1", "work-10"])
        self.assertEqual(s.tagged("t"), ["work-1", "work-10", "work-2"])
        s.delete("work-10")
        self.assertEqual(s.keys(), ["home", "work-1", "work-2"])

    def test_files_on_disk(self):
        s = self.make()
        n = Note("A", "b", ("x",), 7)
        s.put("note-a", n)
        path = os.path.join(self.dir, "note-a.json")
        with open(path) as f:
            self.assertEqual(json.load(f), n.to_dict())
        self.assertEqual(self.make().get("note-a"), n)
        s.delete("note-a")
        self.assertFalse(os.path.exists(path))
        self.assertEqual(self.make().keys(), [])

    def test_creates_directory_and_ignores_other_files(self):
        nested = os.path.join(self.tmp.name, "a", "b", "c")
        s = self.make(nested)
        self.assertTrue(os.path.isdir(nested))
        self.assertEqual(s.keys(), [])
        with open(os.path.join(nested, "README.txt"), "w") as f:
            f.write("not a note")
        s.put("real", Note("Real"))
        self.assertEqual(s.keys(), ["real"])

    def test_open_store(self):
        from notes.store import open_store
        from notes.store.dirstore import DirStore
        s = open_store("dir:" + self.dir)
        self.assertIsInstance(s, DirStore)
        s.put("x", Note("X"))
        self.assertEqual(open_store("dir:" + self.dir).get("x"), Note("X"))
        with self.assertRaises(ValueError):
            open_store("dir:")

    def test_cli(self):
        url = "dir:" + self.dir

        def run(*args):
            return subprocess.run([sys.executable, "-m", "notes", "--store", url, *args],
                                  capture_output=True, text=True, timeout=30)
        self.assertEqual(run("add", "trip", "Trip", "pack bags", "--tag", "travel").returncode, 0)
        self.assertEqual(run("add", "bills", "Bills").returncode, 0)
        self.assertEqual(run("list").stdout, "bills\tBills\ntrip\tTrip\n")
        self.assertEqual(run("show", "trip").stdout, "Trip\ntags: travel\n\npack bags\n")
        self.assertEqual(run("rm", "bills").returncode, 0)
        self.assertEqual(run("show", "bills").returncode, 1)
        self.assertEqual(sorted(os.listdir(self.dir)), ["trip.json"])

    def test_existing_backends_unchanged(self):
        from notes.store import JsonFileStore, MemoryStore, open_store
        self.assertIsInstance(open_store("memory:"), MemoryStore)
        js = open_store("json:" + os.path.join(self.tmp.name, "n.json"))
        self.assertIsInstance(js, JsonFileStore)
        js.put("a", Note("A"))
        self.assertEqual(js.keys(), ["a"])
        with self.assertRaises(ValueError):
            open_store("nope:x")


if __name__ == "__main__":
    unittest.main()
