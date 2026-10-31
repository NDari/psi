"""Discount rules applied before tax."""
from shop.pricing import calc_total


def bulk_discount(lines, threshold=10, percent=5):
    """Lines with at least `threshold` units get `percent` off the unit price."""
    out = []
    for price, qty in lines:
        if qty >= threshold:
            price = price * (100 - percent) // 100
        out.append((price, qty))
    return out


def discounted_total(lines, tax):
    """Order total after the bulk discount."""
    return calc_total(bulk_discount(lines), tax)
