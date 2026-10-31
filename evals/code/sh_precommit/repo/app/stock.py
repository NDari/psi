"""Stock levels for the inventory service."""


class Stock:
    def __init__(self):
        self._levels = {}

    def add(self, sku, n):
        if n <= 0:
            raise ValueError("n must be positive")
        self._levels[sku] = self._levels.get(sku, 0) + n

    def remove(self, sku, n):
        have = self._levels.get(sku, 0)
        if n > have:
            raise ValueError(f"only {have} of {sku} in stock")
        self._levels[sku] = have - n

    def level(self, sku):
        return self._levels.get(sku, 0)
