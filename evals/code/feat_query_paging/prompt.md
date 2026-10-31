The admin pages need sorting and paging in records.py. Add to Query, keeping the rule that builder methods return a new Query and leave the original unchanged:

- `order_by(*fields)`: sort by these fields in order; a field written as "-age" sorts descending. Records with a missing or None value for a field sort after all others for that field, in both directions. The sort is stable. Calling order_by again replaces the earlier ordering.
- `limit(n)` and `offset(n)`: skip the first `offset` records, then return at most `limit`. Negative values raise ValueError.
- `page(number, size)`: return `Page(items, number, size, total, pages)`, a namedtuple named Page defined in records.py. Pages start at 1. items are the shaped records (like `all()`) of that page after filters and ordering; limit and offset are ignored. total is the number of matching records, pages is total divided by size rounded up (0 when total is 0). A number past the last page gives empty items. number or size below 1 raises ValueError.

`all()` and `first()` apply ordering, offset and limit; `count()` keeps counting every matching record.
