"""Queries on books."""


def author_id(conn, name):
    """Id of the author called name (trimmed), creating the author if needed. None for a blank name."""
    name = (name or "").strip()
    if not name:
        return None
    row = conn.execute("SELECT id FROM authors WHERE name = ?", (name,)).fetchone()
    if row:
        return row[0]
    return conn.execute("INSERT INTO authors (name) VALUES (?)", (name,)).lastrowid


def add_book(conn, title, author, year=None):
    """Add a book and return its id."""
    with conn:
        aid = author_id(conn, author)
        cur = conn.execute("INSERT INTO books (title, author_id, year) VALUES (?, ?, ?)", (title, aid, year))
    return cur.lastrowid


def find_by_author(conn, name):
    """Titles of the books by the author called exactly name, sorted."""
    rows = conn.execute("""
        SELECT b.title FROM books b JOIN authors a ON a.id = b.author_id
        WHERE a.name = ? ORDER BY b.title
    """, (name,))
    return [r[0] for r in rows]


def list_books(conn):
    """(id, title, author, year) for every book, sorted by title then id."""
    return conn.execute("""
        SELECT b.id, b.title, a.name, b.year FROM books b LEFT JOIN authors a ON a.id = b.author_id
        ORDER BY b.title, b.id
    """).fetchall()


def books_per_author(conn):
    """(author name, number of books) for authors with at least one book,
    most books first, ties by name."""
    return conn.execute("""
        SELECT a.name, count(*) AS n FROM books b JOIN authors a ON a.id = b.author_id
        GROUP BY a.id ORDER BY n DESC, a.name
    """).fetchall()


def remove_book(conn, book_id):
    with conn:
        n = conn.execute("DELETE FROM books WHERE id = ?", (book_id,)).rowcount
    if n == 0:
        raise KeyError(book_id)
