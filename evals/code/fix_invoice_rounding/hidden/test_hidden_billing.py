import json
import unittest

from billing.invoice import Invoice, from_order
from billing.money import format_money, parse_amount
from billing.tax import line_vat


class TestHiddenBilling(unittest.TestCase):
    def test_parse_amount_exact(self):
        cases = {"0.29": 29, "0.57": 57, "1.15": 115, "4.35": 435, "2.01": 201,
                 "19.99": 1999, "1234.56": 123456, "8.2": 820, "0.07": 7, "11.15": 1115}
        self.assertEqual({k: parse_amount(k) for k in cases}, cases)

    def test_line_vat_half_up(self):
        self.assertEqual(line_vat(150, "7"), 11)     # 10.5
        self.assertEqual(line_vat(350, "7"), 25)     # 24.5
        self.assertEqual(line_vat(250, "5"), 13)     # 12.5
        self.assertEqual(line_vat(1100, "5.5"), 61)  # 60.5
        self.assertEqual(line_vat(50, "19"), 10)     # 9.5

    def test_sample_order_totals(self):
        with open("/hidden/order_0412.json") as f:
            inv = from_order(json.load(f))
        self.assertEqual(inv.net, 2637)
        self.assertEqual(inv.vat_by_rate(), {"19": 473, "7": 11})
        self.assertEqual(inv.total, 3121)

    def test_vat_summed_per_line(self):
        inv = Invoice()
        for name in "ABC":
            inv.add_line(name, 1, "1.50", "7")
        inv.add_line("D", 3, "0.29", "0")
        self.assertEqual(inv.net, 537)
        self.assertEqual(inv.vat_by_rate(), {"7": 33, "0": 0})
        self.assertEqual(inv.total, 570)

    def test_existing_behaviour(self):
        self.assertEqual(format_money(123456), "1,234.56")
        self.assertEqual(format_money(-5), "-0.05")
        self.assertEqual(parse_amount("12"), 1200)
        self.assertEqual(parse_amount(" 12.5 "), 1250)
        for bad in ["1.234", "-1", "abc", "", "1,5"]:
            with self.assertRaises(ValueError, msg=bad):
                parse_amount(bad)
        self.assertEqual(line_vat(1000, "19"), 190)
        self.assertEqual(line_vat(1234, "0"), 0)
        self.assertEqual(line_vat(999, "19"), 190)   # 189.81
        self.assertEqual(line_vat(1001, "5.5"), 55)  # 55.055
        with self.assertRaises(ValueError):
            from_order({"lines": [{"description": "x", "quantity": 0, "unit_price": "1", "vat": "7"}]})


if __name__ == "__main__":
    unittest.main()
