import datetime
import unittest

from ledger import bank_a, bank_b, card, payroll, petty_cash
from ledger.model import Transaction

D = datetime.date


class Importers(unittest.TestCase):
    def test_bank_a(self):
        text = 'date,description,amount\n2024-01-02,Rent,"1,200.00"\n2024-01-05,Refund,15.5\n'
        self.assertEqual(bank_a.load(text), [
            Transaction(D(2024, 1, 2), 120000, "Rent"),
            Transaction(D(2024, 1, 5), 1550, "Refund"),
        ])

    def test_bank_b(self):
        self.assertEqual(bank_b.load("-42.10;03/02/2024;Grocer\n\n"),
                         [Transaction(D(2024, 2, 3), -4210, "Grocer")])

    def test_card(self):
        text = "DATE        DESCRIPTION      AMOUNT\n2024-03-01  Coffee shop  $3.20\n"
        self.assertEqual(card.load(text), [Transaction(D(2024, 3, 1), -320, "Coffee shop")])

    def test_payroll(self):
        text = "date: 2024-04-30\nemployee: Ana\nnet: 2,100.00\n\ndate: 2024-04-30\nemployee: Bo\nnet: 1,950.75\n"
        self.assertEqual([t.amount for t in payroll.load(text)], [-210000, -195075])

    def test_petty_cash(self):
        text = "# April\n2024-04-02\t(7.25)\tstamps\n"
        self.assertEqual(petty_cash.load(text), [Transaction(D(2024, 4, 2), -725, "stamps")])


if __name__ == "__main__":
    unittest.main()
