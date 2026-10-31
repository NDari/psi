import unittest
from datetime import date
from decimal import Decimal

from shipping import (
    Parcel,
    QuoteError,
    chargeable_weight,
    estimate_delivery,
    is_holiday,
    make_tracking_number,
    quote,
    validate_postcode,
)

MONDAY = date(2026, 3, 2)


class ChargeableWeightTest(unittest.TestCase):
    def test_actual_weight_rounded_up(self):
        self.assertEqual(chargeable_weight(Parcel(1.2, 10, 10, 10)), Decimal("1.5"))

    def test_volumetric_weight_wins(self):
        # 50 x 40 x 30 / 5000 = 12 kg
        self.assertEqual(chargeable_weight(Parcel(3, 50, 40, 30)), Decimal("12.0"))

    def test_express_divisor(self):
        # 50 x 40 x 30 / 4000 = 15 kg
        self.assertEqual(chargeable_weight(Parcel(3, 50, 40, 30), "express"), Decimal("15.0"))

    def test_too_heavy(self):
        with self.assertRaises(QuoteError):
            chargeable_weight(Parcel(71, 10, 10, 10))


class QuoteTest(unittest.TestCase):
    def test_domestic_standard(self):
        q = quote("DE", "DE", Parcel(2, 30, 20, 10), ship_date=MONDAY)
        self.assertEqual(q.amount("Base rate"), Decimal("4.80"))
        self.assertEqual(q.total, Decimal("6.55"))

    def test_domestic_insurance(self):
        q = quote("DE", "DE", Parcel(2, 30, 20, 10, declared_value=600), ship_date=MONDAY)
        self.assertEqual(q.amount("Insurance"), Decimal("5.00"))

    def test_included_cover(self):
        q = quote("DE", "FR", Parcel(2, 30, 20, 10, declared_value=100), ship_date=MONDAY)
        self.assertEqual(q.amount("Insurance"), Decimal("0.00"))

    def test_remote_area(self):
        q = quote("DE", "DE", Parcel(2, 30, 20, 10), ship_date=MONDAY, postcode="25938")
        self.assertEqual(q.amount("Remote area"), Decimal("18.00"))

    def test_oversize(self):
        q = quote("FR", "FR", Parcel(5, 130, 20, 20), ship_date=MONDAY)
        self.assertEqual(q.amount("Oversize"), Decimal("25.00"))

    def test_too_large(self):
        with self.assertRaises(QuoteError):
            quote("FR", "FR", Parcel(5, 180, 20, 20), ship_date=MONDAY)

    def test_saturday_only_domestic_express(self):
        with self.assertRaises(QuoteError):
            quote("DE", "FR", Parcel(1, 10, 10, 10), service="express", saturday=True, ship_date=MONDAY)


class CalendarTest(unittest.TestCase):
    def test_easter_monday(self):
        self.assertTrue(is_holiday("DE", date(2026, 4, 6)))
        self.assertFalse(is_holiday("DE", date(2026, 4, 7)))

    def test_delivery_skips_holiday(self):
        # Thursday 2026-04-02, Good Friday and Easter Monday in DE
        self.assertEqual(
            estimate_delivery("DE", "DE", "express", date(2026, 4, 2)),
            (date(2026, 4, 7), date(2026, 4, 7)),
        )


class LabelTest(unittest.TestCase):
    def test_tracking_number(self):
        self.assertEqual(make_tracking_number("express", "de", 12345678), "EX123456785DE")

    def test_postcodes(self):
        self.assertTrue(validate_postcode("NL", "1012 ab"))
        self.assertFalse(validate_postcode("DE", "1234"))


if __name__ == "__main__":
    unittest.main()
