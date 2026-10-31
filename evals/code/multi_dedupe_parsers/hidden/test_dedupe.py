import datetime
import pathlib
import re
import unittest

D = datetime.date


class Dedupe(unittest.TestCase):
    def assertUsesShared(self, name):
        src = pathlib.Path("ledger", name + ".py").read_text()
        self.assertIsNone(re.search(r"def\s+_?parse_(amount|date)\b", src), f"{name}.py still defines its own parser")
        self.assertIn("parsing", src, f"{name}.py does not use ledger.parsing")

    def test_parse_amount(self):
        from ledger.parsing import parse_amount
        cases = {"1,234.50": 123450, "-3": -300, "$12.05": 1205, "(7.25)": -725, " 0.5 ": 50,
                 "-$1,000": -100000, "( 2.00 )": -200, "15": 1500}
        for text, want in cases.items():
            self.assertEqual(parse_amount(text), want, text)
        for bad in ["", "abc", "1.234", "1.2.3", "$", "12a"]:
            with self.assertRaises(ValueError, msg=bad):
                parse_amount(bad)

    def test_parse_date(self):
        from ledger.parsing import parse_date
        self.assertEqual(parse_date("2024-03-09"), D(2024, 3, 9))
        self.assertEqual(parse_date(" 09/03/2024 "), D(2024, 3, 9))
        self.assertEqual(parse_date("29/02/2024"), D(2024, 2, 29))
        for bad in ["2024-13-01", "24-01-01", "01/01/24", "yesterday", "30/02/2024"]:
            with self.assertRaises(ValueError, msg=bad):
                parse_date(bad)

    def test_bank_a(self):
        self.assertUsesShared("bank_a")
        from ledger import bank_a
        text = 'date,description,amount\n2023-12-31,Fee,-2.5\n31/12/2023,Big,"(1,000.01)"\n'
        got = [(t.date, t.amount, t.description) for t in bank_a.load(text)]
        self.assertEqual(got, [(D(2023, 12, 31), -250, "Fee"), (D(2023, 12, 31), -100001, "Big")])

    def test_bank_b(self):
        self.assertUsesShared("bank_b")
        from ledger import bank_b
        got = [(t.date, t.amount, t.description) for t in bank_b.load("$1,005.20;2024-07-01; Client X \n-0.99;01/07/2024;App\n")]
        self.assertEqual(got, [(D(2024, 7, 1), 100520, "Client X"), (D(2024, 7, 1), -99, "App")])
        with self.assertRaises(ValueError):
            bank_b.load("1.999;2024-07-01;x\n")

    def test_card(self):
        self.assertUsesShared("card")
        from ledger import card
        text = "DATE        DESCRIPTION   AMOUNT\n05/06/2024  Train  12\n2024-06-06  Refund shop  -4.10\n"
        self.assertEqual([t.amount for t in card.load(text)], [-1200, 410])

    def test_payroll(self):
        self.assertUsesShared("payroll")
        from ledger import payroll
        got = payroll.load("Date: 31/05/2024\nEmployee: Cy\nNet: $3,000\n")
        self.assertEqual([(t.date, t.amount, t.description) for t in got], [(D(2024, 5, 31), -300000, "salary Cy")])

    def test_petty_cash(self):
        self.assertUsesShared("petty_cash")
        from ledger import petty_cash
        got = petty_cash.load("  # header\n2024-08-01\t20\tfloat\n\n02/08/2024\t-1.5\tmilk\n")
        self.assertEqual([t.amount for t in got], [2000, -150])
        with self.assertRaises(ValueError):
            petty_cash.load("2024-08-32\t1\tx\n")


if __name__ == "__main__":
    unittest.main()
