"""Schema migrations. MIGRATIONS[i] takes a database from version i to i + 1.

Never edit a migration that has shipped; add a new one at the end.
"""


def migrate_1(conn):
    conn.executescript("""
        CREATE TABLE books (
            id INTEGER PRIMARY KEY,
            title TEXT NOT NULL,
            author TEXT,
            year INTEGER
        );
        CREATE TABLE loans (
            id INTEGER PRIMARY KEY,
            book_id INTEGER NOT NULL REFERENCES books(id),
            borrower TEXT NOT NULL,
            returned INTEGER NOT NULL DEFAULT 0
        );
    """)


def migrate_2(conn):
    """Move authors into their own table."""
    conn.executescript("""
        CREATE TABLE authors (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );
        INSERT INTO authors (name)
            SELECT DISTINCT trim(author) FROM books
            WHERE author IS NOT NULL AND trim(author) != ''
            ORDER BY trim(author);
        ALTER TABLE books ADD COLUMN author_id INTEGER REFERENCES authors(id);
        UPDATE books SET author_id = (SELECT id FROM authors WHERE name = trim(books.author));
        ALTER TABLE books DROP COLUMN author;
    """)


MIGRATIONS = [migrate_1, migrate_2]


def version(conn):
    return conn.execute("PRAGMA user_version").fetchone()[0]


def migrate(conn):
    """Apply every migration the database has not had yet, each in its own transaction."""
    for i in range(version(conn), len(MIGRATIONS)):
        with conn:
            MIGRATIONS[i](conn)
            conn.execute(f"PRAGMA user_version = {i + 1}")
