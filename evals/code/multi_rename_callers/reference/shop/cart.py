"""A shopping cart that remembers its lines."""
from . import pricing


class Cart:
    def __init__(self, tax_rate=0.2):
        self.tax_rate = tax_rate
        self.lines = []

    def add(self, price, qty=1):
        if qty <= 0:
            raise ValueError("quantity must be positive")
        self.lines.append((price, qty))

    def item_count(self):
        return sum(qty for _, qty in self.lines)

    def total(self):
        return pricing.order_total(self.lines, tax_rate=self.tax_rate)
