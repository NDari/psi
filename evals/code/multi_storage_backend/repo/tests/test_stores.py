import os
import tempfile
import unittest

from notes.model import Note
from notes.store import JsonFileStore, MemoryStore, open_store


class Contract:
    """Tests every backend must pass. Subclasses define make_store()."""

    def test_roundtrip(self):
        s = self.make_store()
        n = Note("Title", "body", ("a", "b"), 5)
        s.put("k1", n)
        self.assertEqual(s.get("k1"), n)
        self.assertIn("k1", s)
        self.assertEqual(len(s), 1)

    def test_missing(self):
        s = self.make_store()
        with self.assertRaises(KeyError):
            s.get("nope")
        with self.assertRaises(KeyError):
            s.delete("nope")

    def test_invalid_key(self):
        s = self.make_store()
        for bad in ["", "Upper", "-x", "a/b", "x" * 65]:
            with self.assertRaises(ValueError):
                s.put(bad, Note("t"))

    def test_keys_and_search(self):
        s = self.make_store()
        for k in ["b-2", "a-1", "b-1"]:
            s.put(k, Note(k.upper(), "text " + k))
        self.assertEqual(s.keys(), ["a-1", "b-1", "b-2"])
        self.assertEqual(s.keys("b-"), ["b-1", "b-2"])
        self.assertEqual(s.search("TEXT B"), ["b-1", "b-2"])
        s.delete("b-1")
        self.assertEqual(s.keys(), ["a-1", "b-2"])


class MemoryStoreTest(Contract, unittest.TestCase):
    def make_store(self):
        return MemoryStore()


class JsonFileStoreTest(Contract, unittest.TestCase):
    def make_store(self):
        d = tempfile.TemporaryDirectory()
        self.addCleanup(d.cleanup)
        return JsonFileStore(os.path.join(d.name, "notes.json"))


class OpenStoreTest(unittest.TestCase):
    def test_urls(self):
        self.assertIsInstance(open_store("memory:"), MemoryStore)
        self.assertIsInstance(open_store("json:/tmp/x.json"), JsonFileStore)
        with self.assertRaises(ValueError):
            open_store("ftp:x")


if __name__ == "__main__":
    unittest.main()
