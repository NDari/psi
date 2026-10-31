# parcelquote

Prices parcels for our shipping desk: a route, a parcel and a service go in,
a quote with a line by line breakdown and a delivery window comes out.

    python3 cli.py DE FR 2 30x20x10 --value 600 --date 2026-03-02

    from shipping import Parcel, quote, format_quote
    q = quote("DE", "FR", Parcel(2, 30, 20, 10, declared_value=600))
    print(q.total, q.lines)

`quote()` returns a `Quote`; `q.lines` is a list of `(label, amount)` pairs
in the order below and `q.total` is their sum. Amounts are `Decimal`, rounded
half up to cents at every line.

Tests: `python3 -m unittest`

## Rate card

All prices in EUR. The numbers live in the tables in `shipping.py`.

1. **Zone.** `ZONES` maps a destination to a zone per origin (DE, FR, NL,
   GB, US, PL). Zone 0 is domestic; 1 to 8 are international.
2. **Chargeable weight.** The greater of the actual weight and the
   volumetric weight (L x W x H / 5000, or / 4000 for express), rounded up
   to the next 0.5 kg. Parcels over 70 kg actual are refused.
3. **Base rate.** The first band in `RATE_CARDS[service][zone]` whose
   upper limit is at least the chargeable weight. Above 30 kg: the 30 kg
   price plus `EXTRA_KG[service][zone]` per started kg.
4. **Account discount.** bronze 5%, silver 10%, gold 15%, platinum 20% of
   the base rate.
5. **Promo code.** A percentage of the base rate; see `PROMO_CODES` for
   dates, services and minimums.
6. **Remote area.** Destination postcodes in `REMOTE_AREAS`: 18.00 or
   0.35 per chargeable kg, whichever is more.
7. **Oversize.** Longest side over 120 cm or length plus girth
   (longest + 2 x the other two) over 300 cm: 25.00. Over 175 cm or over
   400 cm length plus girth: refused.
8. **Heavy parcel.** Actual weight over 31.5 kg: 15.00.
9. **Fuel.** The month's percentage from `FUEL_SURCHARGE_PCT` (12.50% when
   the month is missing) of: base rate minus discounts plus remote area,
   oversize and heavy parcel charges.
10. **Saturday delivery.** Domestic express only: 12.00.
11. **Insurance.** Every parcel includes cover for a declared value up to
    100.00 at no charge. Above that, insurance costs a percentage of the
    part of the declared value above 100.00: 1.0% domestic, 1.8%
    international, at least 2.50. Declared values above 5000.00 are refused.
12. **Customs clearance.** Parcels with a declared value that cross the EU
    customs border (either end outside the EU): 6.50.
13. **VAT.** Domestic only: the origin country's rate on the sum of all
    lines above.

## Delivery window

`TRANSIT_DAYS[service][zone]` gives the minimum and maximum working days.
The parcel is picked up on the ship date, or the next working day in the
origin country; transit days count working days (Monday to Friday, plus
Saturday with Saturday delivery) in the destination country, skipping its
public holidays.
