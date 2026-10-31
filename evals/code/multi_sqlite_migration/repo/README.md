# library

The book and loan register of the office library, kept in SQLite.

    python3 -m library --db lib.sqlite add "Dune" "Herbert, Frank" --year 1965
    python3 -m library --db lib.sqlite list
    python3 -m library --db lib.sqlite lend 1 alice

The schema is versioned with `PRAGMA user_version`; `library.migrations.migrate`
brings any database (including a new empty one) up to the latest version, and
`library.db.connect` calls it. Tests: `python3 -m unittest discover -s tests`
