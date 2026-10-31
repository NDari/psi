"""Lending books out and taking them back."""


class LoanError(Exception):
    pass


def lend(conn, book_id, borrower):
    """Record a loan and return its id. A book can only be on one open loan."""
    if conn.execute("SELECT 1 FROM books WHERE id = ?", (book_id,)).fetchone() is None:
        raise LoanError(f"no book {book_id}")
    if conn.execute("SELECT 1 FROM loans WHERE book_id = ? AND returned = 0", (book_id,)).fetchone():
        raise LoanError(f"book {book_id} is already on loan")
    with conn:
        cur = conn.execute("INSERT INTO loans (book_id, borrower) VALUES (?, ?)", (book_id, borrower))
    return cur.lastrowid


def give_back(conn, book_id):
    with conn:
        n = conn.execute("UPDATE loans SET returned = 1 WHERE book_id = ? AND returned = 0", (book_id,)).rowcount
    if n == 0:
        raise LoanError(f"book {book_id} is not on loan")


def on_loan(conn):
    """(title, borrower) of every open loan, sorted by title."""
    return conn.execute("""
        SELECT b.title, l.borrower FROM loans l JOIN books b ON b.id = l.book_id
        WHERE l.returned = 0 ORDER BY b.title
    """).fetchall()
