"""Command line: python3 -m library --db PATH COMMAND ..."""
import argparse
import sys

from library import books, loans
from library.db import connect


def main(argv=None):
    ap = argparse.ArgumentParser(prog="library")
    ap.add_argument("--db", default="library.sqlite")
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("add")
    a.add_argument("title")
    a.add_argument("author")
    a.add_argument("--year", type=int)
    sub.add_parser("list")
    by = sub.add_parser("by")
    by.add_argument("author")
    l = sub.add_parser("lend")
    l.add_argument("book_id", type=int)
    l.add_argument("borrower")
    r = sub.add_parser("return")
    r.add_argument("book_id", type=int)
    sub.add_parser("loans")
    args = ap.parse_args(argv)

    conn = connect(args.db)
    try:
        if args.cmd == "add":
            print(books.add_book(conn, args.title, args.author, args.year))
        elif args.cmd == "list":
            for bid, title, author, year in books.list_books(conn):
                print(f"{bid}\t{title}\t{author or '-'}\t{year or '-'}")
        elif args.cmd == "by":
            for title in books.find_by_author(conn, args.author):
                print(title)
        elif args.cmd == "lend":
            loans.lend(conn, args.book_id, args.borrower)
        elif args.cmd == "return":
            loans.give_back(conn, args.book_id)
        elif args.cmd == "loans":
            for title, who in loans.on_loan(conn):
                print(f"{title}\t{who}")
    except loans.LoanError as e:
        print(f"library: {e}", file=sys.stderr)
        return 1
    finally:
        conn.close()
    return 0
