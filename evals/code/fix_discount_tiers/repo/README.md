# shop-pricing

Order pricing for the web shop. All amounts are integer cents.

Volume discount on the order subtotal:

| subtotal         | discount |
|------------------|----------|
| from $100.00     | 5%       |
| from $500.00     | 10%      |
| from $1,000.00   | 15%      |

The discount is rounded down to a whole cent. Shipping is $4.90, free from
$50.00 of subtotal (before the discount).

Run the tests with `python3 -m unittest`.
