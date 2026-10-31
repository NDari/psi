import os
import pathlib
import subprocess
import sys
import unittest

LINES = [(250, 2), (1000, 1)]


def source(mod):
    return pathlib.Path("shop", mod + ".py").read_text()


class Rename(unittest.TestCase):
    def assertUpdated(self, mod):
        self.assertNotIn("calc_total", source(mod), f"shop/{mod}.py still mentions calc_total")

    def test_order_total(self):
        from shop import pricing
        self.assertEqual(pricing.order_total(LINES, 0.2), 1800)
        self.assertEqual(pricing.order_total(LINES, tax_rate=0.1), 1650)
        self.assertEqual(pricing.order_total([], 0.2), 0)

    def test_old_name_gone(self):
        from shop import pricing
        self.assertFalse(hasattr(pricing, "calc_total"), "pricing.calc_total still exists")
        for p in pathlib.Path("shop").rglob("*.py"):
            self.assertNotIn("calc_total", p.read_text(), f"{p} still mentions calc_total")

    def test_cart(self):
        self.assertUpdated("cart")
        from shop.cart import Cart
        c = Cart(tax_rate=0.1)
        c.add(999, 3)
        c.add(1)
        self.assertEqual(c.item_count(), 4)
        self.assertEqual(c.total(), 3298)
        with self.assertRaises(ValueError):
            c.add(5, 0)

    def test_discounts(self):
        self.assertUpdated("discounts")
        from shop.discounts import discounted_total
        self.assertEqual(discounted_total([(200, 10), (100, 1)], 0.2), 2400)
        self.assertEqual(discounted_total([(1000, 10)], 0), 9500)

    def test_invoice(self):
        self.assertUpdated("invoice")
        from shop.invoice import render
        want = "3 x 1.99 = 5.97\nsubtotal 5.97\ntax 0.60\ntotal 6.57\n"
        self.assertEqual(render([(199, 3)], 0.1), want)

    def test_checkout(self):
        self.assertUpdated("checkout")
        from shop.checkout import PaymentError, checkout
        self.assertEqual(checkout(LINES, 0.2, 2000), 200)
        with self.assertRaises(PaymentError):
            checkout(LINES, 0.2, 1799)

    def test_report(self):
        self.assertUpdated("report")
        from shop.report import daily_summary
        self.assertEqual(daily_summary([LINES, [(100, 1)]], 0.2), "orders=2 revenue=19.20 largest=18.00")
        self.assertEqual(daily_summary([], 0.2), "orders=0 revenue=0.00 largest=0.00")

    def test_split(self):
        self.assertUpdated("split")
        from shop.split import split_evenly
        self.assertEqual(split_evenly(LINES, 0.2, 7), [258, 257, 257, 257, 257, 257, 257])

    def test_cli(self):
        self.assertUpdated("cli")
        out = subprocess.run([sys.executable, "-m", "shop.cli", "250x2", "1000x1", "--tax", "0.2"],
                             capture_output=True, text=True, timeout=30, cwd=os.getcwd())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(out.stdout, "2 x 2.50 = 5.00\n1 x 10.00 = 10.00\nsubtotal 15.00\ntax 3.00\ntotal 18.00\ndue 1800 cents\n")


if __name__ == "__main__":
    unittest.main()
