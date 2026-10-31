"""Opening the database."""
import sqlite3

from library.migrations import migrate


def connect(path):
    """Open (or create) the database at path and migrate it to the latest schema."""
    conn = sqlite3.connect(path)
    conn.execute("PRAGMA foreign_keys = ON")
    migrate(conn)
    return conn
