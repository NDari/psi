from typing import NamedTuple
import datetime


class Transaction(NamedTuple):
    date: datetime.date
    amount: int  # cents, negative for money going out
    description: str
