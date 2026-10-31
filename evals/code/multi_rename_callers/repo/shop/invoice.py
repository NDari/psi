"""Plain text invoices."""
from shop.money import fmt
from shop.pricing import calc_total, subtotal


def render(lines, tax):
    """Return the invoice text for an order."""
    out = []
    for price, qty in lines:
        out.append(f"{qty} x {fmt(price)} = {fmt(price * qty)}")
    sub = subtotal(lines)
    total = calc_total(lines, tax)
    out.append(f"subtotal {fmt(sub)}")
    out.append(f"tax {fmt(total - sub)}")
    out.append(f"total {fmt(total)}")
    return "\n".join(out) + "\n"
