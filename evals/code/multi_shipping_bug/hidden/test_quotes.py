import unittest
from datetime import date
from decimal import Decimal

from shipping import Parcel, QuoteError, quote


class InternationalInsuranceTest(unittest.TestCase):
    def test_reported_example(self):
        q = quote("DE", "FR", Parcel(2, 30, 20, 10, declared_value=600), ship_date=date(2026, 3, 2))
        self.assertEqual(q.total, Decimal("17.64"))

    def test_value_250(self):
        q = quote("DE", "AT", Parcel(1.5, 25, 15, 10, declared_value=250), ship_date=date(2026, 6, 15))
        self.assertEqual(q.amount("Insurance"), Decimal("2.70"))
        self.assertEqual(q.total, Decimal("11.38"))

    def test_express_outside_eu(self):
        q = quote("US", "CA", Parcel(1, 20, 20, 20, declared_value=1000), service="express",
                  ship_date=date(2026, 11, 25))
        self.assertEqual(q.amount("Insurance"), Decimal("16.20"))
        self.assertEqual(q.total, Decimal("42.39"))

    def test_highest_value(self):
        q = quote("NL", "GB", Parcel(4, 40, 30, 20, declared_value=5000), service="express",
                  ship_date=date(2026, 10, 30))
        self.assertEqual(q.amount("Insurance"), Decimal("88.20"))
        self.assertEqual(q.total, Decimal("121.50"))

    def test_minimum_premium(self):
        q = quote("FR", "ES", Parcel(3, 30, 30, 20, declared_value=180), service="economy",
                  ship_date=date(2026, 5, 4))
        self.assertEqual(q.amount("Insurance"), Decimal("2.50"))
        self.assertEqual(q.total, Decimal("14.12"))


class UnchangedTest(unittest.TestCase):
    def test_domestic_insured_saturday(self):
        q = quote("GB", "GB", Parcel(6, 40, 30, 30, declared_value=2600), service="express",
                  saturday=True, ship_date=date(2026, 8, 28), postcode="KW1 4AA")
        self.assertEqual(q.amount("Insurance"), Decimal("25.00"))
        self.assertEqual(q.total, Decimal("86.03"))
        self.assertEqual((q.earliest, q.latest), (date(2026, 8, 29), date(2026, 8, 29)))

    def test_uninsured_international_surcharges(self):
        q = quote("DE", "NO", Parcel(35, 130, 40, 30), ship_date=date(2026, 12, 22),
                  postcode="8006", account="gold")
        self.assertEqual(q.amount("Insurance"), Decimal("0.00"))
        self.assertEqual(q.total, Decimal("98.04"))

    def test_value_above_limit_refused(self):
        with self.assertRaises(QuoteError):
            quote("DE", "FR", Parcel(2, 30, 20, 10, declared_value=5000.01), ship_date=date(2026, 3, 2))


if __name__ == "__main__":
    unittest.main()
