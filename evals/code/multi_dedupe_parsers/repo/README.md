# ledger

Importers that turn the exports of our banks and internal sheets into
`Transaction(date, amount, description)` records (`ledger/model.py`).
Amounts are integer cents; dates are `datetime.date`.

Each importer module has a `load(text)` function that takes the whole file
content and returns a list of transactions.

Tests: `python3 -m unittest discover -s tests`
