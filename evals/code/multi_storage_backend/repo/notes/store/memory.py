"""In-memory backend, mostly for tests."""
from notes.store.base import Store, check_key


class MemoryStore(Store):
    def __init__(self):
        self._notes = {}

    def get(self, key):
        check_key(key)
        return self._notes[key]

    def put(self, key, note):
        check_key(key)
        self._notes[key] = note

    def delete(self, key):
        check_key(key)
        del self._notes[key]

    def keys(self, prefix=""):
        return sorted(k for k in self._notes if k.startswith(prefix))
