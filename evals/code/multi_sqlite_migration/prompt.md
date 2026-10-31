The library database stores the author as free text in `books.author`. Move authors into their own table with a new schema migration, version 2, in library/migrations.py (don't edit migration 1).

- New table `authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)`, and `books` gets `author_id` referencing it. The old `books.author` column is dropped. Book ids and the loans table stay as they are.
- Existing data moves over: one author row per distinct name after trimming spaces; books with a NULL or blank author get `author_id` NULL. `connect()` on a version 1 database must end at `PRAGMA user_version` 2, and running `migrate` again does nothing.
- Update library/books.py to the new schema without changing what the functions return: `add_book(conn, title, author, year=None)` reuses an existing author or creates one (name trimmed, blank means no author), `find_by_author`, and `list_books` (which still returns `(id, title, author_name_or_None, year)`).
- Add `books_per_author(conn)` to library/books.py: a list of `(author_name, book_count)` for authors with at least one book, most books first, ties by name.

Tests are in tests/ (`python3 -m unittest discover -s tests`).
