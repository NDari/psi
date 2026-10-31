The shipping desk says some international quotes come out too high. Example:

    quote("DE", "FR", Parcel(2, 30, 20, 10, declared_value=600), ship_date=date(2026, 3, 2))

gives a total of 19.44 (same with `python3 cli.py DE FR 2 30x20x10 --value 600 --date 2026-03-02`),
but working it out by hand from the rate card in README.md gives 17.64. Domestic quotes look right.

Please find the cause in shipping.py and fix it. Don't change any other pricing or the
public interface: `quote(...)` returns a `Quote` whose `.total` and `.lines` amounts are
Decimals, `format_quote(q)` prints the breakdown, and `cli.py` keeps its arguments.
Tests are in tests/, run `python3 -m unittest`.
