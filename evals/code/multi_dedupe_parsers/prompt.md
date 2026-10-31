Every importer in ledger/ (bank_a, bank_b, card, payroll, petty_cash) carries its own copy of `_parse_amount` and `_parse_date`. Pull them into one new module, `ledger/parsing.py`, as public functions `parse_amount(text)` (returns integer cents) and `parse_date(text)` (returns a `datetime.date`), and make every importer import and use those instead. Delete the private copies from the importers.

Behaviour must not change at all: same accepted formats, same results, same `ValueError` on bad input. The tests in tests/ should keep passing (`python3 -m unittest discover -s tests`).
