import unittest

from pricing import discount, discount_percent, order_total, shipping


class TestPricing(unittest.TestCase):
    def test_small_order_has_no_discount(self):
        self.assertEqual(discount_percent(4_000), 0)

    def test_tiers(self):
        self.assertEqual(discount_percent(25_000), 5)
        self.assertEqual(discount_percent(75_000), 10)
        self.assertEqual(discount_percent(250_000), 15)

    def test_exactly_100_gets_5_percent(self):
        self.assertEqual(discount_percent(10_000), 5)

    def test_discount_rounds_down(self):
        self.assertEqual(discount(10_019), 500)

    def test_shipping(self):
        self.assertEqual(shipping(4_999), 490)
        self.assertEqual(shipping(5_000), 0)

    def test_order_total(self):
        self.assertEqual(order_total([(2, 1_500)]), 3_490)
        self.assertEqual(order_total([(3, 10_000)]), 28_500)


if __name__ == "__main__":
    unittest.main()
