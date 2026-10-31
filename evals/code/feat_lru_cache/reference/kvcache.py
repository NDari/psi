"""An in-memory key/value cache with optional per-entry expiry.

    cache = Cache()
    cache.set("user:1", {"name": "Ada"}, ttl=30)
    cache.get("user:1")

Time comes from the `clock` function given to the constructor (default
time.monotonic), so tests can control it.
"""
import time
from collections import OrderedDict

_MISSING = object()


class Cache:
    def __init__(self, clock=time.monotonic, max_size=None):
        if max_size is not None and max_size < 1:
            raise ValueError("max_size must be at least 1")
        self._clock = clock
        self._max_size = max_size
        # key -> (value, expires_at or None), least recently used first
        self._data = OrderedDict()
        self._hits = 0
        self._misses = 0
        self._evictions = 0

    def _live(self, key):
        """Return the entry for key, or None; drops it if it has expired."""
        entry = self._data.get(key)
        if entry is None:
            return None
        expires_at = entry[1]
        if expires_at is not None and self._clock() >= expires_at:
            del self._data[key]
            return None
        return entry

    def get(self, key, default=None):
        """Return the value for key, or default when it is absent or expired."""
        entry = self._live(key)
        if entry is None:
            self._misses += 1
            return default
        self._hits += 1
        self._data.move_to_end(key)
        return entry[0]

    def set(self, key, value, ttl=None):
        """Store value under key. With ttl (seconds), it expires that long from now."""
        if ttl is not None and ttl <= 0:
            raise ValueError("ttl must be positive")
        expires_at = None if ttl is None else self._clock() + ttl
        if key in self._data:
            self._data.move_to_end(key)
        elif self._max_size is not None and len(self._data) >= self._max_size:
            self.purge()
            if len(self._data) >= self._max_size:
                self._data.popitem(last=False)
                self._evictions += 1
        self._data[key] = (value, expires_at)

    def delete(self, key):
        """Remove key. Returns True when a live entry was removed."""
        if self._live(key) is None:
            return False
        del self._data[key]
        return True

    def purge(self):
        """Drop every expired entry. Returns how many were dropped."""
        now = self._clock()
        dead = [k for k, (_, exp) in self._data.items() if exp is not None and now >= exp]
        for k in dead:
            del self._data[k]
        return len(dead)

    def clear(self):
        self._data.clear()

    def __contains__(self, key):
        return self._live(key) is not None

    def __len__(self):
        self.purge()
        return len(self._data)

    def stats(self):
        return {"hits": self._hits, "misses": self._misses, "size": len(self),
                "evictions": self._evictions}
