"""Backend that keeps every note in one JSON file: {"key": note_dict, ...}."""
import json
import os

from notes.model import Note
from notes.store.base import Store, check_key


class JsonFileStore(Store):
    def __init__(self, path):
        self.path = path

    def _load(self):
        try:
            with open(self.path, encoding="utf-8") as f:
                return json.load(f)
        except FileNotFoundError:
            return {}

    def _save(self, data):
        tmp = self.path + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2, sort_keys=True)
        os.replace(tmp, self.path)

    def get(self, key):
        check_key(key)
        data = self._load()
        if key not in data:
            raise KeyError(key)
        return Note.from_dict(data[key])

    def put(self, key, note):
        check_key(key)
        data = self._load()
        data[key] = note.to_dict()
        self._save(data)

    def delete(self, key):
        check_key(key)
        data = self._load()
        if key not in data:
            raise KeyError(key)
        del data[key]
        self._save(data)

    def keys(self, prefix=""):
        return sorted(k for k in self._load() if k.startswith(prefix))
