"""Core price arithmetic. All amounts are integer cents."""


def subtotal(lines):
    """Sum of unit price times quantity over all order lines."""
    return sum(price * qty for price, qty in lines)


def tax_amount(amount, rate):
    """Tax on amount at rate (0.2 means 20%), rounded half up to a cent."""
    return int(amount * rate + 0.5)


def order_total(lines, tax_rate):
    """Total of an order: subtotal plus tax at rate `tax_rate`."""
    sub = subtotal(lines)
    return sub + tax_amount(sub, tax_rate)
