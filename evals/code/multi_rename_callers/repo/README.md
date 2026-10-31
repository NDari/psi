# shop

A tiny order-pricing library used by the till and the nightly report.

Prices are integer cents. An order line is a `(unit_price_cents, quantity)` tuple.

    python3 -m shop.cli 250x2 1000x1 --tax 0.2

prints the receipt for two items at 2.50 and one at 10.00 with 20% tax.
