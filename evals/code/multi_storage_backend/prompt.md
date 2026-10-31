Add a third storage backend to notes, next to MemoryStore and JsonFileStore: `DirStore` in a new module `notes/store/dirstore.py`. It keeps each note in its own file, `<dir>/<key>.json`, holding the note's `to_dict()` as JSON.

- `DirStore(path)`: the constructor creates the directory (with parents) if it does not exist.
- It must follow the `Store` contract in notes/store/base.py exactly like the other two backends: same key validation, `KeyError` for missing keys, `keys(prefix="")` sorted. `keys()` only counts `*.json` files and ignores anything else in the directory.
- Notes written by one `DirStore` are visible to a new `DirStore` on the same directory.
- Register it in `open_store` as `dir:PATH` (an empty path raises `ValueError`, like `json:`), so `python3 -m notes --store dir:/some/dir list` works.

Tests are in tests/ (`python3 -m unittest discover -s tests`).
