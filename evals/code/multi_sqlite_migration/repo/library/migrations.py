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


MIGRATIONS = [migrate_1]


def version(conn):
    return conn.execute("PRAGMA user_version").fetchone()[0]


def migrate(conn):
    """Apply every migration the database has not had yet, each in its own transaction."""
    for i in range(version(conn), len(MIGRATIONS)):
        with conn:
            MIGRATIONS[i](conn)
            conn.execute(f"PRAGMA user_version = {i + 1}")
