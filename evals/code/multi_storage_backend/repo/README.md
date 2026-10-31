# notes

A small note keeper with pluggable storage backends.

    python3 -m notes --store json:/tmp/notes.json add shopping "Shopping" "milk, eggs" --tag home
    python3 -m notes --store json:/tmp/notes.json list
    python3 -m notes --store json:/tmp/notes.json show shopping

Store URLs (see `notes/store/__init__.py`):

- `memory:` keeps notes in memory (tests),
- `json:PATH` keeps every note in one JSON file.

Every backend implements `notes.store.base.Store`; read its docstrings for the
contract. Tests: `python3 -m unittest discover -s tests`
