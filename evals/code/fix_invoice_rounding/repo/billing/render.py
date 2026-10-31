"""Plain-text invoice layout."""
from .money import format_money

WIDTH = 60


def _row(left, right):
    return f"{left:<{WIDTH - 14}}{right:>14}"


def render(inv):
    out = [f"INVOICE {inv.number}".rstrip(), f"Customer: {inv.customer}", ""]
    for line in inv.lines:
        label = f"{line.quantity} x {line.description} @ {format_money(line.unit_price)} ({line.rate}%)"
        out.append(_row(label, format_money(line.net)))
    out.append("-" * WIDTH)
    out.append(_row("Net", format_money(inv.net)))
    for rate, vat in inv.vat_by_rate().items():
        out.append(_row(f"VAT {rate}%", format_money(vat)))
    out.append(_row("TOTAL", format_money(inv.total)))
    return "\n".join(out)
