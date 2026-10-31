"""A tiny query builder over a list of dict records.

    q = Query(people).where(city="Oslo").where_fn(lambda r: r["age"] >= 18)
    q.select("name", "age").all()

Every builder method returns a new Query; the one it was called on is left
unchanged, so a base query can be shared and refined in several ways.
"""


class Query:
    def __init__(self, records):
        self._records = list(records)
        self._filters = []
        self._fields = None

    def _copy(self):
        q = Query.__new__(Query)
        q._records = self._records
        q._filters = list(self._filters)
        q._fields = self._fields
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

    def _matching(self):
        return [r for r in self._records if all(f(r) for f in self._filters)]

    def _shape(self, records):
        if self._fields is None:
            return [dict(r) for r in records]
        return [{f: r.get(f) for f in self._fields} for r in records]

    def all(self):
        """Return the matching records as new dicts."""
        return self._shape(self._matching())

    def first(self):
        """Return the first matching record, or None."""
        rows = self.all()
        return rows[0] if rows else None

    def count(self):
        """Return the number of matching records."""
        return len(self._matching())
