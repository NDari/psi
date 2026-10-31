"""Checkout: take payment for an order."""
import shop.pricing


class PaymentError(Exception):
    pass


def checkout(lines, tax, paid):
    """Return the change due in cents, or raise PaymentError if paid is short."""
    due = shop.pricing.calc_total(lines, tax)
    if paid < due:
        raise PaymentError(f"short by {due - paid} cents")
    return paid - due
