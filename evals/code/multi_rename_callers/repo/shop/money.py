"""Formatting of cent amounts."""


def fmt(cents):
    """Format cents as a decimal string, for example 1234 -> '12.34'."""
    sign = "-" if cents < 0 else ""
    cents = abs(cents)
    return f"{sign}{cents // 100}.{cents % 100:02d}"
