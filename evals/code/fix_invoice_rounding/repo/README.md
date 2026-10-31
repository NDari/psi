# billing

Turns an order (JSON) into a printable invoice.

    python3 -m billing orders/2026-0412.json

## Order format

    {
      "number": "2026-0412",
      "customer": "Acme GmbH",
      "lines": [
        {"description": "Widget", "quantity": 3, "unit_price": "0.29", "vat": "19"}
      ]
    }

`unit_price` is a decimal string with at most two decimals ("12", "12.5",
"12.50"). `vat` is the VAT rate in percent, as a string ("19", "7", "5.5",
"0").

## Rules

All amounts are kept as integer cents.

- line net = quantity x unit price
- line VAT = line net x rate / 100, rounded to a whole cent, halves
  rounded up (0.5 cent -> 1 cent)
- the invoice shows the net total, the VAT per rate (sum of the line VATs
  at that rate) and the total = net total + all VAT

Run the tests with `python3 -m unittest`.
