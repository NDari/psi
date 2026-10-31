"""Queries on books."""


def add_book(conn, title, author, year=None):
    """Add a book and return its id."""
    with conn:
        cur = conn.execute("INSERT INTO books (title, author, year) VALUES (?, ?, ?)", (title, author, year))
    return cur.lastrowid


def find_by_author(conn, name):
    """Titles of the books by the author called exactly name, sorted."""
    rows = conn.execute("SELECT title FROM books WHERE author = ? ORDER BY title", (name,))
    return [r[0] for r in rows]


def list_books(conn):
    """(id, title, author, year) for every book, sorted by title then id."""
    return conn.execute("SELECT id, title, author, year FROM books ORDER BY title, id").fetchall()


def remove_book(conn, book_id):
    with conn:
        n = conn.execute("DELETE FROM books WHERE id = ?", (book_id,)).rowcount
    if n == 0:
        raise KeyError(book_id)
