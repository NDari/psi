"""Nightly sales report."""
from shop import pricing as p
from shop.money import fmt


def daily_summary(orders, tax):
    """orders is a list of order line lists. Returns one summary line."""
    totals = [p.order_total(lines, tax) for lines in orders]
    count = len(totals)
    revenue = sum(totals)
    biggest = max(totals) if totals else 0
    return f"orders={count} revenue={fmt(revenue)} largest={fmt(biggest)}"
