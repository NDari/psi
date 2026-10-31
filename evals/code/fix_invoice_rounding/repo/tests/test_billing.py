import unittest

from billing.invoice import Invoice
from billing.money import format_money, parse_amount
from billing.tax import line_vat


class TestBilling(unittest.TestCase):
    def test_parse_amount(self):
        self.assertEqual(parse_amount("12"), 1200)
        self.assertEqual(parse_amount("12.5"), 1250)
        self.assertEqual(parse_amount("2.40"), 240)
        with self.assertRaises(ValueError):
            parse_amount("1.234")

    def test_format_money(self):
        self.assertEqual(format_money(123456), "1,234.56")
        self.assertEqual(format_money(5), "0.05")

    def test_line_vat(self):
        self.assertEqual(line_vat(1000, "19"), 190)
        self.assertEqual(line_vat(1000, "0"), 0)

    def test_invoice_totals(self):
        inv = Invoice().add_line("A", 2, "10.00", "19").add_line("B", 1, "5.00", "7")
        self.assertEqual((inv.net, inv.vat, inv.total), (2500, 415, 2915))
        self.assertEqual(inv.vat_by_rate(), {"19": 380, "7": 35})


if __name__ == "__main__":
    unittest.main()
