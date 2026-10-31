import os
import sqlite3
import subprocess
import sys
import tempfile
import unittest

V1 = """
CREATE TABLE books (id INTEGER PRIMARY KEY, title TEXT NOT NULL, author TEXT, year INTEGER);
CREATE TABLE loans (id INTEGER PRIMARY KEY, book_id INTEGER NOT NULL REFERENCES books(id),
                    borrower TEXT NOT NULL, returned INTEGER NOT NULL DEFAULT 0);
INSERT INTO books (id, title, author, year) VALUES
  (1, 'Dune', 'Herbert, Frank', 1965),
  (2, 'Emma', 'Austen, Jane', 1815),
  (5, 'Children of Dune', '  Herbert, Frank ', 1976),
  (7, 'Persuasion', 'Austen, Jane', NULL),
  (8, 'Anonymous Poems', NULL, NULL),
  (9, 'Notes', '   ', 2001),
  (11, 'Solaris', 'Lem, Stanislaw', 1961);
INSERT INTO loans (book_id, borrower, returned) VALUES (2, 'ana', 0), (1, 'bo', 1);
PRAGMA user_version = 1;
"""


def columns(conn, table):
    return [r[1] for r in conn.execute(f"PRAGMA table_info({table})")]


class MigrationTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = os.path.join(self.tmp.name, "lib.sqlite")

    def v1(self):
        c = sqlite3.connect(self.path)
        c.executescript(V1)
        c.close()

    def open(self):
        from library.db import connect
        conn = connect(self.path)
        self.addCleanup(conn.close)
        return conn

    def test_fresh_database(self):
        conn = self.open()
        self.assertEqual(conn.execute("PRAGMA user_version").fetchone()[0], 2)
        self.assertEqual(sorted(columns(conn, "authors")), ["id", "name"])
        self.assertEqual(sorted(columns(conn, "books")), ["author_id", "id", "title", "year"])

    def test_migrates_v1_data(self):
        self.v1()
        conn = self.open()
        self.assertEqual(conn.execute("PRAGMA user_version").fetchone()[0], 2)
        self.assertNotIn("author", columns(conn, "books"))
        names = [r[0] for r in conn.execute("SELECT name FROM authors ORDER BY name")]
        self.assertEqual(names, ["Austen, Jane", "Herbert, Frank", "Lem, Stanislaw"])
        rows = conn.execute("""SELECT b.id, b.title, a.name, b.year FROM books b
                               LEFT JOIN authors a ON a.id = b.author_id ORDER BY b.id""").fetchall()
        self.assertEqual(rows, [(1, "Dune", "Herbert, Frank", 1965), (2, "Emma", "Austen, Jane", 1815),
                                (5, "Children of Dune", "Herbert, Frank", 1976), (7, "Persuasion", "Austen, Jane", None),
                                (8, "Anonymous Poems", None, None), (9, "Notes", None, 2001),
                                (11, "Solaris", "Lem, Stanislaw", 1961)])

    def test_author_names_unique(self):
        conn = self.open()
        conn.execute("INSERT INTO authors (name) VALUES ('X')")
        with self.assertRaises(sqlite3.IntegrityError):
            conn.execute("INSERT INTO authors (name) VALUES ('X')")

    def test_migrate_twice(self):
        self.v1()
        from library.migrations import migrate
        conn = self.open()
        migrate(conn)
        conn.close()
        conn = self.open()
        self.assertEqual(conn.execute("SELECT count(*) FROM authors").fetchone()[0], 3)
        self.assertEqual(conn.execute("SELECT count(*) FROM books").fetchone()[0], 7)

    def test_add_book(self):
        from library import books
        self.v1()
        conn = self.open()
        bid = books.add_book(conn, "Fiasco", " Lem, Stanislaw", 1986)
        books.add_book(conn, "Ubik", "Dick, Philip K.")
        books.add_book(conn, "Untitled", "")
        self.assertEqual(conn.execute("SELECT count(*) FROM authors").fetchone()[0], 4)
        self.assertEqual(books.find_by_author(conn, "Lem, Stanislaw"), ["Fiasco", "Solaris"])
        self.assertEqual(books.find_by_author(conn, "Dick, Philip K."), ["Ubik"])
        self.assertEqual(books.find_by_author(conn, "Nobody"), [])
        self.assertIn((bid, "Fiasco", "Lem, Stanislaw", 1986), books.list_books(conn))
        self.assertIn("Untitled", [r[1] for r in books.list_books(conn) if r[2] is None])

    def test_list_books(self):
        from library import books
        self.v1()
        conn = self.open()
        self.assertEqual(books.list_books(conn), [
            (8, "Anonymous Poems", None, None), (5, "Children of Dune", "Herbert, Frank", 1976),
            (1, "Dune", "Herbert, Frank", 1965), (2, "Emma", "Austen, Jane", 1815),
            (9, "Notes", None, 2001), (7, "Persuasion", "Austen, Jane", None),
            (11, "Solaris", "Lem, Stanislaw", 1961)])

    def test_books_per_author(self):
        from library import books
        self.v1()
        conn = self.open()
        books.add_book(conn, "Ubik", "Dick, Philip K.")
        conn.execute("INSERT INTO authors (name) VALUES ('Nobody Yet')")
        self.assertEqual([tuple(r) for r in books.books_per_author(conn)],
                         [("Austen, Jane", 2), ("Herbert, Frank", 2), ("Dick, Philip K.", 1), ("Lem, Stanislaw", 1)])
        self.assertEqual(list(books.books_per_author(self.fresh())), [])

    def fresh(self):
        from library.db import connect
        conn = connect(":memory:")
        self.addCleanup(conn.close)
        return conn

    def test_loans_and_cli_after_migration(self):
        from library import loans
        self.v1()
        out = subprocess.run([sys.executable, "-m", "library", "--db", self.path, "list"],
                             capture_output=True, text=True, timeout=30)
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(out.stdout.splitlines()[:3], ["8\tAnonymous Poems\t-\t-", "5\tChildren of Dune\tHerbert, Frank\t1976",
                                                       "1\tDune\tHerbert, Frank\t1965"])
        conn = self.open()
        self.assertEqual(loans.on_loan(conn), [("Emma", "ana")])
        loans.lend(conn, 11, "cy")
        self.assertEqual(loans.on_loan(conn), [("Emma", "ana"), ("Solaris", "cy")])


if __name__ == "__main__":
    unittest.main()
