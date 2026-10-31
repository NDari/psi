import unittest

from library import books, loans
from library.db import connect


class LibraryTest(unittest.TestCase):
    def setUp(self):
        self.conn = connect(":memory:")
        self.addCleanup(self.conn.close)

    def test_books(self):
        a = books.add_book(self.conn, "Dune", "Herbert, Frank", 1965)
        books.add_book(self.conn, "Children of Dune", "Herbert, Frank")
        books.add_book(self.conn, "Emma", "Austen, Jane", 1815)
        self.assertEqual(books.find_by_author(self.conn, "Herbert, Frank"), ["Children of Dune", "Dune"])
        self.assertEqual(books.list_books(self.conn)[1], (a, "Dune", "Herbert, Frank", 1965))

    def test_loans(self):
        b = books.add_book(self.conn, "Emma", "Austen, Jane")
        loans.lend(self.conn, b, "ana")
        with self.assertRaises(loans.LoanError):
            loans.lend(self.conn, b, "bo")
        self.assertEqual(loans.on_loan(self.conn), [("Emma", "ana")])
        loans.give_back(self.conn, b)
        self.assertEqual(loans.on_loan(self.conn), [])


if __name__ == "__main__":
    unittest.main()
