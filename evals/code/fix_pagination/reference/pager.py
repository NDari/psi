"""Page-number pagination for list endpoints.

Pages are numbered from 1. A page past the last one is empty (not an error).
"""


class Page:
    def __init__(self, items, page, per_page, total):
        self.items = items
        self.page = page
        self.per_page = per_page
        self.total = total

    @property
    def total_pages(self):
        """Number of pages needed for all items (0 when there are none)."""
        return -(-self.total // self.per_page)

    @property
    def has_next(self):
        return self.page < self.total_pages

    @property
    def has_prev(self):
        return self.page > 1

    def to_dict(self):
        return {
            "items": list(self.items),
            "page": self.page,
            "per_page": self.per_page,
            "total": self.total,
            "total_pages": self.total_pages,
            "next_page": self.page + 1 if self.has_next else None,
            "prev_page": self.page - 1 if self.has_prev else None,
        }


def paginate(items, page=1, per_page=20):
    """Return the Page with number `page` of the sequence `items`."""
    if page < 1:
        raise ValueError("page must be 1 or more")
    if per_page < 1:
        raise ValueError("per_page must be 1 or more")
    start = (page - 1) * per_page
    return Page(items[start:start + per_page], page, per_page, len(items))


def fetch_all(fetch_page):
    """Collect the items of every page. fetch_page(n) returns a Page."""
    items = []
    number = 1
    while True:
        page = fetch_page(number)
        items.extend(page.items)
        if not page.has_next:
            return items
        number += 1
