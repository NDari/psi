In the shop package, rename `pricing.calc_total(lines, tax)` to `order_total(lines, tax_rate)`: both the function and its second parameter get the new names. Update every caller in shop/ so everything keeps working exactly as before.

Don't leave an alias or a wrapper under the old name: afterwards no file in shop/ should mention `calc_total` at all. Other functions keep their names and signatures (for example `discounted_total(lines, tax)` and `Cart(tax_rate=...)` stay as they are).
