"""A tiny query builder over a list of dict records.

    q = Query(people).where(city="Oslo").where_fn(lambda r: r["age"] >= 18)
    q.select("name", "age").all()

Every builder method returns a new Query; the one it was called on is left
unchanged, so a base query can be shared and refined in several ways.
"""
from collections import namedtuple

Page = namedtuple("Page", "items number size total pages")


class Query:
    def __init__(self, records):
        self._records = list(records)
        self._filters = []
        self._fields = None
        self._order = ()
        self._offset = 0
        self._limit = None

    def _copy(self):
        q = Query.__new__(Query)
        q._records = self._records
        q._filters = list(self._filters)
        q._fields = self._fields
        q._order = self._order
        q._offset = self._offset
        q._limit = self._limit
        return q

    def where(self, **conditions):
        """Keep records whose fields equal the given values.

        A missing field never matches. A list or tuple value means "one of".
        """
        def match(record):
            for field, want in conditions.items():
                if field not in record:
                    return False
                if isinstance(want, (list, tuple)):
                    if record[field] not in want:
                        return False
                elif record[field] != want:
                    return False
            return True

        q = self._copy()
        q._filters.append(match)
        return q

    def where_fn(self, predicate):
        """Keep records for which predicate(record) is true."""
        q = self._copy()
        q._filters.append(predicate)
        return q

    def select(self, *fields):
        """Return only these fields of each record (missing ones become None)."""
        q = self._copy()
        q._fields = fields
        return q

    def order_by(self, *fields):
        """Sort by fields; "-field" sorts descending. Missing or None values go last."""
        q = self._copy()
        q._order = fields
        return q

    def offset(self, n):
        """Skip the first n records."""
        if n < 0:
            raise ValueError("offset must not be negative")
        q = self._copy()
        q._offset = n
        return q

    def limit(self, n):
        """Return at most n records."""
        if n < 0:
            raise ValueError("limit must not be negative")
        q = self._copy()
        q._limit = n
        return q

    def _sorted(self, records):
        for spec in reversed(self._order):
            desc = spec.startswith("-")
            field = spec[1:] if desc else spec
            present = [r for r in records if r.get(field) is not None]
            missing = [r for r in records if r.get(field) is None]
            present.sort(key=lambda r: r[field], reverse=desc)
            records = present + missing
        return records

    def _window(self):
        rows = self._sorted(self._matching())[self._offset:]
        if self._limit is not None:
            rows = rows[:self._limit]
        return rows

    def page(self, number, size):
        """Return page `number` (from 1) of `size` records, ignoring limit and offset."""
        if number < 1 or size < 1:
            raise ValueError("page number and size must be at least 1")
        rows = self._sorted(self._matching())
        total = len(rows)
        pages = -(-total // size)
        start = (number - 1) * size
        return Page(self._shape(rows[start:start + size]), number, size, total, pages)

    def _matching(self):
        return [r for r in self._records if all(f(r) for f in self._filters)]

    def _shape(self, records):
        if self._fields is None:
            return [dict(r) for r in records]
        return [{f: r.get(f) for f in self._fields} for r in records]

    def all(self):
        """Return the matching records as new dicts."""
        return self._shape(self._window())

    def first(self):
        """Return the first matching record, or None."""
        rows = self.all()
        return rows[0] if rows else None

    def count(self):
        """Return the number of matching records."""
        return len(self._matching())
