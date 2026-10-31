import unittest

import pricing


class TestHiddenPricing(unittest.TestCase):
    def test_boundary_100(self):
        self.assertEqual(pricing.discount_percent(10_000), 5)
        self.assertEqual(pricing.discount(10_000), 500)

    def test_boundary_500(self):
        self.assertEqual(pricing.discount_percent(50_000), 10)

    def test_boundary_1000(self):
        self.assertEqual(pricing.discount_percent(100_000), 15)

    def test_order_total_at_boundaries(self):
        # 4 x $25.00 = $100.00 -> $95.00, free shipping
        self.assertEqual(pricing.order_total([(4, 2_500)]), 9_500)
        # $400 + $100 = $500.00 -> $450.00
        self.assertEqual(pricing.order_total([(1, 40_000), (2, 5_000)]), 45_000)

    def test_existing_behaviour(self):
        self.assertEqual(pricing.discount_percent(9_999), 0)
        self.assertEqual(pricing.discount_percent(49_999), 5)
        self.assertEqual(pricing.discount_percent(99_999), 10)
        self.assertEqual(pricing.discount_percent(0), 0)
        self.assertEqual(pricing.discount(10_019), 500)
        self.assertEqual(pricing.shipping(4_999), 490)
        self.assertEqual(pricing.shipping(5_000), 0)
        self.assertEqual(pricing.order_total([(2, 1_500)]), 3_490)
        self.assertEqual(pricing.order_total([(1, 4_000)]), 4_490)


if __name__ == "__main__":
    unittest.main()
