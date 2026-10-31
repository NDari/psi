"""Storage backends and the store URL factory."""
from notes.store.base import Store, check_key
from notes.store.dirstore import DirStore
from notes.store.jsonfile import JsonFileStore
from notes.store.memory import MemoryStore


def open_store(url):
    """Open a store from a URL: "memory:", "json:PATH" or "dir:PATH"."""
    scheme, sep, rest = url.partition(":")
    if not sep:
        raise ValueError(f"store URL needs a scheme: {url!r}")
    if scheme == "memory":
        return MemoryStore()
    if scheme == "json":
        if not rest:
            raise ValueError("json: store needs a path")
        return JsonFileStore(rest)
    if scheme == "dir":
        if not rest:
            raise ValueError("dir: store needs a path")
        return DirStore(rest)
    raise ValueError(f"unknown store scheme: {scheme!r}")
