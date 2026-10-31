Some of our invoices are off by a cent or a few. Example: orders/2026-0412.json should come to a net of 26.37, VAT 19% of 4.73, VAT 7% of 0.11 and a total of 31.21, but `python3 -m billing orders/2026-0412.json` prints 26.34, 4.72, 0.10 and 31.16. The rules are in README.md (integer cents, line VAT rounded half up).

Please find and fix the cause(s) in the billing package. Keep the existing API (parse_amount, format_money, line_vat, Invoice, from_order, render) and the invoice layout as they are. Tests: python3 -m unittest.
