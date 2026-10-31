"""Backend that keeps each note in its own file: DIR/<key>.json."""
import json
import os

from notes.model import Note
from notes.store.base import Store, check_key


class DirStore(Store):
    def __init__(self, path):
        self.path = path
        os.makedirs(path, exist_ok=True)

    def _file(self, key):
        return os.path.join(self.path, key + ".json")

    def get(self, key):
        check_key(key)
        try:
            with open(self._file(key), encoding="utf-8") as f:
                return Note.from_dict(json.load(f))
        except FileNotFoundError:
            raise KeyError(key) from None

    def put(self, key, note):
        check_key(key)
        tmp = self._file(key) + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(note.to_dict(), f, indent=2, sort_keys=True)
        os.replace(tmp, self._file(key))

    def delete(self, key):
        check_key(key)
        try:
            os.remove(self._file(key))
        except FileNotFoundError:
            raise KeyError(key) from None

    def keys(self, prefix=""):
        names = (n[:-5] for n in os.listdir(self.path) if n.endswith(".json"))
        return sorted(k for k in names if k.startswith(prefix))
