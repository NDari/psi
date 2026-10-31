"""Invoices and their totals."""
from dataclasses import dataclass, field

from .money import parse_amount
from .tax import check_rate, line_vat


@dataclass
class Line:
    description: str
    quantity: int
    unit_price: int  # cents
    rate: str  # VAT percent

    @property
    def net(self):
        return self.quantity * self.unit_price

    @property
    def vat(self):
        return line_vat(self.net, self.rate)


@dataclass
class Invoice:
    number: str = ""
    customer: str = ""
    lines: list = field(default_factory=list)

    def add_line(self, description, quantity, unit_price, rate):
        """Add a line; unit_price and rate are strings as in the order JSON."""
        if not isinstance(quantity, int) or quantity < 1:
            raise ValueError(f"bad quantity: {quantity!r}")
        self.lines.append(Line(description, quantity, parse_amount(unit_price), check_rate(rate)))
        return self

    @property
    def net(self):
        return sum(line.net for line in self.lines)

    def vat_by_rate(self):
        """{rate: VAT cents} for each rate used, in order of first use."""
        result = {}
        for line in self.lines:
            result[line.rate] = result.get(line.rate, 0) + line.vat
        return result

    @property
    def vat(self):
        return sum(self.vat_by_rate().values())

    @property
    def total(self):
        return self.net + self.vat


def from_order(order):
    """Build an Invoice from a parsed order dict."""
    inv = Invoice(order.get("number", ""), order.get("customer", ""))
    for i, line in enumerate(order.get("lines", []), start=1):
        try:
            inv.add_line(line["description"], line["quantity"], line["unit_price"], line["vat"])
        except (KeyError, ValueError) as e:
            raise ValueError(f"line {i}: {e}") from None
    return inv
