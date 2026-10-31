"""Order pricing. All amounts are integer cents."""

# (minimum subtotal in cents, discount percent), biggest tier first
TIERS = [
    (100_000, 15),
    (50_000, 10),
    (10_000, 5),
]

SHIPPING = 490
FREE_SHIPPING_FROM = 5_000


def subtotal(lines):
    """Sum of quantity * unit price over (quantity, unit_price_cents) lines."""
    return sum(qty * price for qty, price in lines)


def discount_percent(amount):
    """Volume discount percent for a subtotal in cents."""
    for minimum, percent in TIERS:
        if amount >= minimum:
            return percent
    return 0


def discount(amount):
    """Discount in cents, rounded down to a whole cent."""
    return amount * discount_percent(amount) // 100


def shipping(amount):
    return 0 if amount >= FREE_SHIPPING_FROM else SHIPPING


def order_total(lines):
    """Total to charge for an order: subtotal - discount + shipping."""
    sub = subtotal(lines)
    return sub - discount(sub) + shipping(sub)
