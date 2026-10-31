"""The storage contract every backend implements."""
import re

KEY_RE = re.compile(r"[a-z0-9][a-z0-9_-]{0,63}")


def check_key(key):
    """Raise ValueError unless key is 1 to 64 chars of a-z, 0-9, _ and -, not starting with _ or -."""
    if not isinstance(key, str) or not KEY_RE.fullmatch(key):
        raise ValueError(f"invalid key: {key!r}")


class Store:
    """A mapping from keys to Note objects.

    Backends implement get, put, delete and keys. Every method that takes a
    key calls check_key first, so an invalid key raises ValueError even when
    the store is empty.
    """

    def get(self, key):
        """Return the Note stored under key; raise KeyError if there is none."""
        raise NotImplementedError

    def put(self, key, note):
        """Store note under key, replacing any note already there."""
        raise NotImplementedError

    def delete(self, key):
        """Remove the note under key; raise KeyError if there is none."""
        raise NotImplementedError

    def keys(self, prefix=""):
        """Return the keys that start with prefix, sorted."""
        raise NotImplementedError

    # Shared behaviour built on the four methods above.

    def __contains__(self, key):
        try:
            self.get(key)
        except KeyError:
            return False
        return True

    def __len__(self):
        return len(self.keys())

    def search(self, text):
        """Keys of notes whose title or body contains text (case-insensitive), sorted."""
        text = text.lower()
        return [k for k in self.keys() if text in self.get(k).title.lower() or text in self.get(k).body.lower()]

    def tagged(self, tag):
        """Keys of notes carrying tag, sorted."""
        return [k for k in self.keys() if tag in self.get(k).tags]
