"""Parcel shipping quotes.

This module turns a parcel, a route and a service into a priced quote with
a line by line breakdown and an estimated delivery window. The pricing rules
are described in README.md (the "rate card"); the tables below hold the
numbers.

Public entry points:

    quote(origin, destination, parcel, service="standard", ...) -> Quote
    format_quote(q) -> str
    estimate_delivery(origin, destination, service, ship_date, saturday=False)
    validate_postcode(country, postcode) / normalize_postcode(country, postcode)
    make_tracking_number(service, origin, serial)
    format_label(sender, recipient, q, serial)

All money is handled as Decimal and rounded half up to whole cents.
"""

from __future__ import annotations

import math
import re
from dataclasses import dataclass, field
from datetime import date, timedelta
from decimal import ROUND_CEILING, ROUND_HALF_UP, Decimal

__all__ = [
    "Address",
    "Country",
    "Parcel",
    "Quote",
    "QuoteError",
    "chargeable_weight",
    "estimate_delivery",
    "format_label",
    "format_quote",
    "is_holiday",
    "make_tracking_number",
    "normalize_postcode",
    "quote",
    "validate_postcode",
    "zone_for",
]

CURRENCY = "EUR"
CENT = Decimal("0.01")
ZERO = Decimal("0.00")
HALF_KG = Decimal("0.5")

SERVICES = ("economy", "standard", "express")
ORIGINS = ("DE", "FR", "NL", "GB", "US", "PL")


class QuoteError(ValueError):
    """A parcel or route that cannot be quoted."""


def money(value) -> Decimal:
    """Round a number to whole cents, half up."""
    if not isinstance(value, Decimal):
        value = Decimal(str(value))
    return value.quantize(CENT, rounding=ROUND_HALF_UP)


def _dec(value, what: str) -> Decimal:
    """Convert a user supplied number to Decimal, rejecting junk."""
    if isinstance(value, bool):
        raise QuoteError(f"{what} must be a number")
    if isinstance(value, Decimal):
        out = value
    elif isinstance(value, (int, float)):
        out = Decimal(str(value))
    elif isinstance(value, str):
        try:
            out = Decimal(value.strip())
        except Exception:
            raise QuoteError(f"{what} must be a number, got {value!r}") from None
    else:
        raise QuoteError(f"{what} must be a number")
    if not out.is_finite():
        raise QuoteError(f"{what} must be finite")
    return out


# ---------------------------------------------------------------------------
# Countries and zones
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Country:
    iso: str
    name: str
    region: str
    eu: bool
    postcode_pattern: str


COUNTRIES = {
    "AT": Country("AT", "Austria", "central", True, r"\d{4}"),
    "BE": Country("BE", "Belgium", "west", True, r"\d{4}"),
    "BG": Country("BG", "Bulgaria", "southeast", True, r"\d{4}"),
    "CH": Country("CH", "Switzerland", "central", False, r"\d{4}"),
    "CY": Country("CY", "Cyprus", "southeast", True, r"\d{4}"),
    "CZ": Country("CZ", "Czechia", "central", True, r"\d{3} ?\d{2}"),
    "DE": Country("DE", "Germany", "central", True, r"\d{5}"),
    "DK": Country("DK", "Denmark", "north", True, r"\d{4}"),
    "EE": Country("EE", "Estonia", "baltic", True, r"\d{5}"),
    "ES": Country("ES", "Spain", "south", True, r"\d{5}"),
    "FI": Country("FI", "Finland", "north", True, r"\d{5}"),
    "FR": Country("FR", "France", "west", True, r"\d{5}"),
    "GB": Country("GB", "United Kingdom", "uk", False, r"[A-Z]{1,2}\d[A-Z\d]? ?\d[A-Z]{2}"),
    "GR": Country("GR", "Greece", "southeast", True, r"\d{3} ?\d{2}"),
    "HR": Country("HR", "Croatia", "southeast", True, r"\d{5}"),
    "HU": Country("HU", "Hungary", "central", True, r"\d{4}"),
    "IE": Country("IE", "Ireland", "uk", True, r"[A-Z]\d{2} ?[A-Z\d]{4}"),
    "IS": Country("IS", "Iceland", "north", False, r"\d{3}"),
    "IT": Country("IT", "Italy", "south", True, r"\d{5}"),
    "LT": Country("LT", "Lithuania", "baltic", True, r"(LT-)?\d{5}"),
    "LU": Country("LU", "Luxembourg", "west", True, r"(L-)?\d{4}"),
    "LV": Country("LV", "Latvia", "baltic", True, r"(LV-)?\d{4}"),
    "MT": Country("MT", "Malta", "south", True, r"[A-Z]{3} ?\d{4}"),
    "NL": Country("NL", "Netherlands", "west", True, r"\d{4} ?[A-Z]{2}"),
    "NO": Country("NO", "Norway", "north", False, r"\d{4}"),
    "PL": Country("PL", "Poland", "central", True, r"\d{2}-\d{3}"),
    "PT": Country("PT", "Portugal", "south", True, r"\d{4}-\d{3}"),
    "RO": Country("RO", "Romania", "southeast", True, r"\d{6}"),
    "RS": Country("RS", "Serbia", "southeast", False, r"\d{5}"),
    "SE": Country("SE", "Sweden", "north", True, r"\d{3} ?\d{2}"),
    "SI": Country("SI", "Slovenia", "central", True, r"\d{4}"),
    "SK": Country("SK", "Slovakia", "central", True, r"\d{3} ?\d{2}"),
    "TR": Country("TR", "Turkey", "southeast", False, r"\d{5}"),
    "UA": Country("UA", "Ukraine", "east", False, r"\d{5}"),
    "MD": Country("MD", "Moldova", "east", False, r"(MD-)?\d{4}"),
    "US": Country("US", "United States", "namerica", False, r"\d{5}(-\d{4})?"),
    "CA": Country("CA", "Canada", "namerica", False, r"[A-Z]\d[A-Z] ?\d[A-Z]\d"),
    "MX": Country("MX", "Mexico", "namerica", False, r"\d{5}"),
    "BR": Country("BR", "Brazil", "samerica", False, r"\d{5}-?\d{3}"),
    "AR": Country("AR", "Argentina", "samerica", False, r"[A-Z]?\d{4}[A-Z]{0,3}"),
    "CL": Country("CL", "Chile", "samerica", False, r"\d{7}"),
    "CO": Country("CO", "Colombia", "samerica", False, r"\d{6}"),
    "PE": Country("PE", "Peru", "samerica", False, r"\d{5}"),
    "JP": Country("JP", "Japan", "asia", False, r"\d{3}-?\d{4}"),
    "KR": Country("KR", "South Korea", "asia", False, r"\d{5}"),
    "CN": Country("CN", "China", "asia", False, r"\d{6}"),
    "HK": Country("HK", "Hong Kong", "asia", False, r""),
    "SG": Country("SG", "Singapore", "asia", False, r"\d{6}"),
    "IN": Country("IN", "India", "asia", False, r"\d{6}"),
    "TH": Country("TH", "Thailand", "asia", False, r"\d{5}"),
    "VN": Country("VN", "Vietnam", "asia", False, r"\d{6}"),
    "MY": Country("MY", "Malaysia", "asia", False, r"\d{5}"),
    "ID": Country("ID", "Indonesia", "asia", False, r"\d{5}"),
    "PH": Country("PH", "Philippines", "asia", False, r"\d{4}"),
    "AU": Country("AU", "Australia", "oceania", False, r"\d{4}"),
    "NZ": Country("NZ", "New Zealand", "oceania", False, r"\d{4}"),
    "ZA": Country("ZA", "South Africa", "africa", False, r"\d{4}"),
    "EG": Country("EG", "Egypt", "africa", False, r"\d{5}"),
    "MA": Country("MA", "Morocco", "africa", False, r"\d{5}"),
    "KE": Country("KE", "Kenya", "africa", False, r"\d{5}"),
    "NG": Country("NG", "Nigeria", "africa", False, r"\d{6}"),
    "AE": Country("AE", "United Arab Emirates", "mideast", False, r""),
    "SA": Country("SA", "Saudi Arabia", "mideast", False, r"\d{5}(-\d{4})?"),
    "IL": Country("IL", "Israel", "mideast", False, r"\d{7}"),
    "QA": Country("QA", "Qatar", "mideast", False, r""),
}


# Destination zone by origin. Columns follow ORIGINS; zone 0 is domestic.
ZONES = {
    #       DE  FR  NL  GB  US  PL
    "AT": (1, 1, 1, 3, 5, 1),  # Austria
    "BE": (1, 1, 1, 2, 5, 1),  # Belgium
    "BG": (3, 3, 3, 4, 5, 3),  # Bulgaria
    "CH": (1, 1, 1, 3, 5, 1),  # Switzerland
    "CY": (3, 3, 3, 4, 5, 3),  # Cyprus
    "CZ": (1, 1, 1, 3, 5, 1),  # Czechia
    "DE": (0, 1, 1, 3, 5, 1),  # Germany
    "DK": (2, 2, 2, 3, 5, 2),  # Denmark
    "EE": (2, 3, 3, 3, 5, 2),  # Estonia
    "ES": (2, 2, 2, 3, 5, 2),  # Spain
    "FI": (2, 2, 2, 3, 5, 2),  # Finland
    "FR": (1, 0, 1, 2, 5, 1),  # France
    "GB": (3, 2, 2, 0, 5, 3),  # United Kingdom
    "GR": (3, 3, 3, 4, 5, 3),  # Greece
    "HR": (3, 3, 3, 4, 5, 3),  # Croatia
    "HU": (1, 1, 1, 3, 5, 1),  # Hungary
    "IE": (3, 2, 2, 1, 5, 3),  # Ireland
    "IS": (2, 2, 2, 3, 5, 2),  # Iceland
    "IT": (2, 2, 2, 3, 5, 2),  # Italy
    "LT": (2, 3, 3, 3, 5, 2),  # Lithuania
    "LU": (1, 1, 1, 2, 5, 1),  # Luxembourg
    "LV": (2, 3, 3, 3, 5, 2),  # Latvia
    "MT": (2, 2, 2, 3, 5, 2),  # Malta
    "NL": (1, 1, 0, 2, 5, 1),  # Netherlands
    "NO": (2, 2, 2, 3, 5, 2),  # Norway
    "PL": (1, 1, 1, 3, 5, 0),  # Poland
    "PT": (2, 2, 2, 3, 5, 2),  # Portugal
    "RO": (3, 3, 3, 4, 5, 3),  # Romania
    "RS": (3, 3, 3, 4, 5, 3),  # Serbia
    "SE": (2, 2, 2, 3, 5, 2),  # Sweden
    "SI": (1, 1, 1, 3, 5, 1),  # Slovenia
    "SK": (1, 1, 1, 3, 5, 1),  # Slovakia
    "TR": (3, 3, 3, 4, 5, 3),  # Turkey
    "UA": (4, 4, 4, 5, 5, 4),  # Ukraine
    "MD": (4, 4, 4, 5, 5, 4),  # Moldova
    "US": (6, 6, 6, 6, 0, 6),  # United States
    "CA": (6, 6, 6, 6, 2, 6),  # Canada
    "MX": (6, 6, 6, 6, 2, 6),  # Mexico
    "BR": (8, 8, 8, 8, 5, 8),  # Brazil
    "AR": (8, 8, 8, 8, 5, 8),  # Argentina
    "CL": (8, 8, 8, 8, 5, 8),  # Chile
    "CO": (8, 8, 8, 8, 5, 8),  # Colombia
    "PE": (8, 8, 8, 8, 5, 8),  # Peru
    "JP": (7, 7, 7, 7, 6, 7),  # Japan
    "KR": (7, 7, 7, 7, 6, 7),  # South Korea
    "CN": (7, 7, 7, 7, 6, 7),  # China
    "HK": (7, 7, 7, 7, 6, 7),  # Hong Kong
    "SG": (7, 7, 7, 7, 6, 7),  # Singapore
    "IN": (7, 7, 7, 7, 6, 7),  # India
    "TH": (7, 7, 7, 7, 6, 7),  # Thailand
    "VN": (7, 7, 7, 7, 6, 7),  # Vietnam
    "MY": (7, 7, 7, 7, 6, 7),  # Malaysia
    "ID": (7, 7, 7, 7, 6, 7),  # Indonesia
    "PH": (7, 7, 7, 7, 6, 7),  # Philippines
    "AU": (8, 8, 8, 8, 7, 8),  # Australia
    "NZ": (8, 8, 8, 8, 7, 8),  # New Zealand
    "ZA": (7, 7, 7, 7, 8, 7),  # South Africa
    "EG": (7, 7, 7, 7, 8, 7),  # Egypt
    "MA": (7, 7, 7, 7, 8, 7),  # Morocco
    "KE": (7, 7, 7, 7, 8, 7),  # Kenya
    "NG": (7, 7, 7, 7, 8, 7),  # Nigeria
    "AE": (6, 6, 6, 6, 7, 6),  # United Arab Emirates
    "SA": (6, 6, 6, 6, 7, 6),  # Saudi Arabia
    "IL": (6, 6, 6, 6, 7, 6),  # Israel
    "QA": (6, 6, 6, 6, 7, 6),  # Qatar
}


def zone_for(origin: str, destination: str) -> int:
    """Zone of a route: 0 for domestic, 1 to 8 for international."""
    origin = origin.upper()
    destination = destination.upper()
    if origin not in ORIGINS:
        raise QuoteError(f"we do not ship from {origin}")
    if destination not in ZONES:
        raise QuoteError(f"we do not ship to {destination}")
    return ZONES[destination][ORIGINS.index(origin)]


def is_international(origin: str, destination: str) -> bool:
    return origin.upper() != destination.upper()


def needs_customs(origin: str, destination: str) -> bool:
    """True when the parcel leaves or enters the EU customs union."""
    a = COUNTRIES[origin.upper()]
    b = COUNTRIES[destination.upper()]
    if a.iso == b.iso:
        return False
    return not (a.eu and b.eu)


# ---------------------------------------------------------------------------
# Rate cards
# ---------------------------------------------------------------------------

# Base rate by service and zone: (up to kg, price) bands, then a price
# per started kg above the last band (EXTRA_KG).
RATE_CARDS = {
    "economy": {
        0: (
            (Decimal("0.5"), Decimal("3.55")),
            (Decimal("1"), Decimal("3.65")),
            (Decimal("2"), Decimal("3.95")),
            (Decimal("3"), Decimal("4.25")),
            (Decimal("5"), Decimal("4.75")),
            (Decimal("7.5"), Decimal("5.40")),
            (Decimal("10"), Decimal("6.00")),
            (Decimal("15"), Decimal("7.25")),
            (Decimal("20"), Decimal("8.40")),
            (Decimal("25"), Decimal("9.60")),
            (Decimal("30"), Decimal("10.75")),
        ),
        1: (
            (Decimal("0.5"), Decimal("5.55")),
            (Decimal("1"), Decimal("5.75")),
            (Decimal("2"), Decimal("6.20")),
            (Decimal("3"), Decimal("6.65")),
            (Decimal("5"), Decimal("7.45")),
            (Decimal("7.5"), Decimal("8.45")),
            (Decimal("10"), Decimal("9.40")),
            (Decimal("15"), Decimal("11.30")),
            (Decimal("20"), Decimal("13.15")),
            (Decimal("25"), Decimal("14.95")),
            (Decimal("30"), Decimal("16.75")),
        ),
        2: (
            (Decimal("0.5"), Decimal("7.55")),
            (Decimal("1"), Decimal("7.85")),
            (Decimal("2"), Decimal("8.45")),
            (Decimal("3"), Decimal("9.05")),
            (Decimal("5"), Decimal("10.15")),
            (Decimal("7.5"), Decimal("11.50")),
            (Decimal("10"), Decimal("12.80")),
            (Decimal("15"), Decimal("15.35")),
            (Decimal("20"), Decimal("17.85")),
            (Decimal("25"), Decimal("20.30")),
            (Decimal("30"), Decimal("22.75")),
        ),
        3: (
            (Decimal("0.5"), Decimal("9.60")),
            (Decimal("1"), Decimal("9.95")),
            (Decimal("2"), Decimal("10.70")),
            (Decimal("3"), Decimal("11.45")),
            (Decimal("5"), Decimal("12.85")),
            (Decimal("7.5"), Decimal("14.55")),
            (Decimal("10"), Decimal("16.20")),
            (Decimal("15"), Decimal("19.40")),
            (Decimal("20"), Decimal("22.55")),
            (Decimal("25"), Decimal("25.65")),
            (Decimal("30"), Decimal("28.75")),
        ),
        4: (
            (Decimal("0.5"), Decimal("11.60")),
            (Decimal("1"), Decimal("12.05")),
            (Decimal("2"), Decimal("13.00")),
            (Decimal("3"), Decimal("13.85")),
            (Decimal("5"), Decimal("15.55")),
            (Decimal("7.5"), Decimal("17.60")),
            (Decimal("10"), Decimal("19.60")),
            (Decimal("15"), Decimal("23.50")),
            (Decimal("20"), Decimal("27.30")),
            (Decimal("25"), Decimal("31.05")),
            (Decimal("30"), Decimal("34.70")),
        ),
        5: (
            (Decimal("0.5"), Decimal("13.60")),
            (Decimal("1"), Decimal("14.15")),
            (Decimal("2"), Decimal("15.25")),
            (Decimal("3"), Decimal("16.25")),
            (Decimal("5"), Decimal("18.25")),
            (Decimal("7.5"), Decimal("20.65")),
            (Decimal("10"), Decimal("23.00")),
            (Decimal("15"), Decimal("27.55")),
            (Decimal("20"), Decimal("32.00")),
            (Decimal("25"), Decimal("36.40")),
            (Decimal("30"), Decimal("40.70")),
        ),
        6: (
            (Decimal("0.5"), Decimal("15.65")),
            (Decimal("1"), Decimal("16.25")),
            (Decimal("2"), Decimal("17.50")),
            (Decimal("3"), Decimal("18.65")),
            (Decimal("5"), Decimal("20.95")),
            (Decimal("7.5"), Decimal("23.70")),
            (Decimal("10"), Decimal("26.35")),
            (Decimal("15"), Decimal("31.60")),
            (Decimal("20"), Decimal("36.75")),
            (Decimal("25"), Decimal("41.75")),
            (Decimal("30"), Decimal("46.70")),
        ),
        7: (
            (Decimal("0.5"), Decimal("17.65")),
            (Decimal("1"), Decimal("18.35")),
            (Decimal("2"), Decimal("19.75")),
            (Decimal("3"), Decimal("21.05")),
            (Decimal("5"), Decimal("23.65")),
            (Decimal("7.5"), Decimal("26.75")),
            (Decimal("10"), Decimal("29.75")),
            (Decimal("15"), Decimal("35.70")),
            (Decimal("20"), Decimal("41.45")),
            (Decimal("25"), Decimal("47.10")),
            (Decimal("30"), Decimal("52.70")),
        ),
        8: (
            (Decimal("0.5"), Decimal("19.65")),
            (Decimal("1"), Decimal("20.45")),
            (Decimal("2"), Decimal("22.00")),
            (Decimal("3"), Decimal("23.45")),
            (Decimal("5"), Decimal("26.30")),
            (Decimal("7.5"), Decimal("29.80")),
            (Decimal("10"), Decimal("33.15")),
            (Decimal("15"), Decimal("39.75")),
            (Decimal("20"), Decimal("46.15")),
            (Decimal("25"), Decimal("52.50")),
            (Decimal("30"), Decimal("58.70")),
        ),
    },
    "standard": {
        0: (
            (Decimal("0.5"), Decimal("4.30")),
            (Decimal("1"), Decimal("4.50")),
            (Decimal("2"), Decimal("4.80")),
            (Decimal("3"), Decimal("5.15")),
            (Decimal("5"), Decimal("5.80")),
            (Decimal("7.5"), Decimal("6.60")),
            (Decimal("10"), Decimal("7.35")),
            (Decimal("15"), Decimal("8.80")),
            (Decimal("20"), Decimal("10.25")),
            (Decimal("25"), Decimal("11.70")),
            (Decimal("30"), Decimal("13.10")),
        ),
        1: (
            (Decimal("0.5"), Decimal("6.75")),
            (Decimal("1"), Decimal("7.05")),
            (Decimal("2"), Decimal("7.55")),
            (Decimal("3"), Decimal("8.10")),
            (Decimal("5"), Decimal("9.10")),
            (Decimal("7.5"), Decimal("10.30")),
            (Decimal("10"), Decimal("11.45")),
            (Decimal("15"), Decimal("13.75")),
            (Decimal("20"), Decimal("16.00")),
            (Decimal("25"), Decimal("18.20")),
            (Decimal("30"), Decimal("20.40")),
        ),
        2: (
            (Decimal("0.5"), Decimal("9.20")),
            (Decimal("1"), Decimal("9.60")),
            (Decimal("2"), Decimal("10.30")),
            (Decimal("3"), Decimal("11.00")),
            (Decimal("5"), Decimal("12.35")),
            (Decimal("7.5"), Decimal("14.00")),
            (Decimal("10"), Decimal("15.60")),
            (Decimal("15"), Decimal("18.75")),
            (Decimal("20"), Decimal("21.75")),
            (Decimal("25"), Decimal("24.75")),
            (Decimal("30"), Decimal("27.70")),
        ),
        3: (
            (Decimal("0.5"), Decimal("11.70")),
            (Decimal("1"), Decimal("12.15")),
            (Decimal("2"), Decimal("13.05")),
            (Decimal("3"), Decimal("13.95")),
            (Decimal("5"), Decimal("15.65")),
            (Decimal("7.5"), Decimal("17.75")),
            (Decimal("10"), Decimal("19.75")),
            (Decimal("15"), Decimal("23.70")),
            (Decimal("20"), Decimal("27.55")),
            (Decimal("25"), Decimal("31.30")),
            (Decimal("30"), Decimal("35.05")),
        ),
        4: (
            (Decimal("0.5"), Decimal("14.15")),
            (Decimal("1"), Decimal("14.70")),
            (Decimal("2"), Decimal("15.80")),
            (Decimal("3"), Decimal("16.90")),
            (Decimal("5"), Decimal("18.95")),
            (Decimal("7.5"), Decimal("21.45")),
            (Decimal("10"), Decimal("23.90")),
            (Decimal("15"), Decimal("28.65")),
            (Decimal("20"), Decimal("33.30")),
            (Decimal("25"), Decimal("37.85")),
            (Decimal("30"), Decimal("42.35")),
        ),
        5: (
            (Decimal("0.5"), Decimal("16.60")),
            (Decimal("1"), Decimal("17.30")),
            (Decimal("2"), Decimal("18.55")),
            (Decimal("3"), Decimal("19.80")),
            (Decimal("5"), Decimal("22.25")),
            (Decimal("7.5"), Decimal("25.15")),
            (Decimal("10"), Decimal("28.00")),
            (Decimal("15"), Decimal("33.60")),
            (Decimal("20"), Decimal("39.05")),
            (Decimal("25"), Decimal("44.40")),
            (Decimal("30"), Decimal("49.65")),
        ),
        6: (
            (Decimal("0.5"), Decimal("19.05")),
            (Decimal("1"), Decimal("19.85")),
            (Decimal("2"), Decimal("21.30")),
            (Decimal("3"), Decimal("22.75")),
            (Decimal("5"), Decimal("25.55")),
            (Decimal("7.5"), Decimal("28.90")),
            (Decimal("10"), Decimal("32.15")),
            (Decimal("15"), Decimal("38.55")),
            (Decimal("20"), Decimal("44.80")),
            (Decimal("25"), Decimal("50.95")),
            (Decimal("30"), Decimal("57.00")),
        ),
        7: (
            (Decimal("0.5"), Decimal("21.50")),
            (Decimal("1"), Decimal("22.40")),
            (Decimal("2"), Decimal("24.05")),
            (Decimal("3"), Decimal("25.70")),
            (Decimal("5"), Decimal("28.80")),
            (Decimal("7.5"), Decimal("32.60")),
            (Decimal("10"), Decimal("36.30")),
            (Decimal("15"), Decimal("43.50")),
            (Decimal("20"), Decimal("50.55")),
            (Decimal("25"), Decimal("57.45")),
            (Decimal("30"), Decimal("64.30")),
        ),
        8: (
            (Decimal("0.5"), Decimal("24.00")),
            (Decimal("1"), Decimal("24.95")),
            (Decimal("2"), Decimal("26.80")),
            (Decimal("3"), Decimal("28.60")),
            (Decimal("5"), Decimal("32.10")),
            (Decimal("7.5"), Decimal("36.30")),
            (Decimal("10"), Decimal("40.45")),
            (Decimal("15"), Decimal("48.45")),
            (Decimal("20"), Decimal("56.30")),
            (Decimal("25"), Decimal("64.00")),
            (Decimal("30"), Decimal("71.60")),
        ),
    },
    "express": {
        0: (
            (Decimal("0.5"), Decimal("7.10")),
            (Decimal("1"), Decimal("7.40")),
            (Decimal("2"), Decimal("7.95")),
            (Decimal("3"), Decimal("8.50")),
            (Decimal("5"), Decimal("9.55")),
            (Decimal("7.5"), Decimal("10.85")),
            (Decimal("10"), Decimal("12.10")),
            (Decimal("15"), Decimal("14.55")),
            (Decimal("20"), Decimal("16.95")),
            (Decimal("25"), Decimal("19.30")),
            (Decimal("30"), Decimal("21.60")),
        ),
        1: (
            (Decimal("0.5"), Decimal("11.15")),
            (Decimal("1"), Decimal("11.60")),
            (Decimal("2"), Decimal("12.50")),
            (Decimal("3"), Decimal("13.35")),
            (Decimal("5"), Decimal("15.00")),
            (Decimal("7.5"), Decimal("17.00")),
            (Decimal("10"), Decimal("18.95")),
            (Decimal("15"), Decimal("22.70")),
            (Decimal("20"), Decimal("26.45")),
            (Decimal("25"), Decimal("30.05")),
            (Decimal("30"), Decimal("33.65")),
        ),
        2: (
            (Decimal("0.5"), Decimal("15.20")),
            (Decimal("1"), Decimal("15.85")),
            (Decimal("2"), Decimal("17.05")),
            (Decimal("3"), Decimal("18.20")),
            (Decimal("5"), Decimal("20.40")),
            (Decimal("7.5"), Decimal("23.10")),
            (Decimal("10"), Decimal("25.75")),
            (Decimal("15"), Decimal("30.90")),
            (Decimal("20"), Decimal("35.95")),
            (Decimal("25"), Decimal("40.85")),
            (Decimal("30"), Decimal("45.75")),
        ),
        3: (
            (Decimal("0.5"), Decimal("19.25")),
            (Decimal("1"), Decimal("20.05")),
            (Decimal("2"), Decimal("21.55")),
            (Decimal("3"), Decimal("23.05")),
            (Decimal("5"), Decimal("25.85")),
            (Decimal("7.5"), Decimal("29.25")),
            (Decimal("10"), Decimal("32.60")),
            (Decimal("15"), Decimal("39.10")),
            (Decimal("20"), Decimal("45.40")),
            (Decimal("25"), Decimal("51.65")),
            (Decimal("30"), Decimal("57.80")),
        ),
        4: (
            (Decimal("0.5"), Decimal("23.35")),
            (Decimal("1"), Decimal("24.30")),
            (Decimal("2"), Decimal("26.10")),
            (Decimal("3"), Decimal("27.85")),
            (Decimal("5"), Decimal("31.25")),
            (Decimal("7.5"), Decimal("35.40")),
            (Decimal("10"), Decimal("39.40")),
            (Decimal("15"), Decimal("47.25")),
            (Decimal("20"), Decimal("54.90")),
            (Decimal("25"), Decimal("62.45")),
            (Decimal("30"), Decimal("69.85")),
        ),
        5: (
            (Decimal("0.5"), Decimal("27.40")),
            (Decimal("1"), Decimal("28.50")),
            (Decimal("2"), Decimal("30.65")),
            (Decimal("3"), Decimal("32.70")),
            (Decimal("5"), Decimal("36.70")),
            (Decimal("7.5"), Decimal("41.50")),
            (Decimal("10"), Decimal("46.25")),
            (Decimal("15"), Decimal("55.45")),
            (Decimal("20"), Decimal("64.40")),
            (Decimal("25"), Decimal("73.25")),
            (Decimal("30"), Decimal("81.95")),
        ),
        6: (
            (Decimal("0.5"), Decimal("31.45")),
            (Decimal("1"), Decimal("32.75")),
            (Decimal("2"), Decimal("35.20")),
            (Decimal("3"), Decimal("37.55")),
            (Decimal("5"), Decimal("42.10")),
            (Decimal("7.5"), Decimal("47.65")),
            (Decimal("10"), Decimal("53.05")),
            (Decimal("15"), Decimal("63.60")),
            (Decimal("20"), Decimal("73.90")),
            (Decimal("25"), Decimal("84.05")),
            (Decimal("30"), Decimal("94.00")),
        ),
        7: (
            (Decimal("0.5"), Decimal("35.50")),
            (Decimal("1"), Decimal("36.95")),
            (Decimal("2"), Decimal("39.70")),
            (Decimal("3"), Decimal("42.40")),
            (Decimal("5"), Decimal("47.55")),
            (Decimal("7.5"), Decimal("53.80")),
            (Decimal("10"), Decimal("59.90")),
            (Decimal("15"), Decimal("71.80")),
            (Decimal("20"), Decimal("83.40")),
            (Decimal("25"), Decimal("94.80")),
            (Decimal("30"), Decimal("106.10")),
        ),
        8: (
            (Decimal("0.5"), Decimal("39.55")),
            (Decimal("1"), Decimal("41.20")),
            (Decimal("2"), Decimal("44.25")),
            (Decimal("3"), Decimal("47.25")),
            (Decimal("5"), Decimal("52.95")),
            (Decimal("7.5"), Decimal("59.90")),
            (Decimal("10"), Decimal("66.70")),
            (Decimal("15"), Decimal("79.95")),
            (Decimal("20"), Decimal("92.90")),
            (Decimal("25"), Decimal("105.60")),
            (Decimal("30"), Decimal("118.15")),
        ),
    },
}

EXTRA_KG = {
    "economy": {0: Decimal("0.30"), 1: Decimal("0.45"), 2: Decimal("0.60"), 3: Decimal("0.75"), 4: Decimal("0.90"), 5: Decimal("1.05"), 6: Decimal("1.20"), 7: Decimal("1.35"), 8: Decimal("1.50")},
    "standard": {0: Decimal("0.35"), 1: Decimal("0.55"), 2: Decimal("0.70"), 3: Decimal("0.90"), 4: Decimal("1.10"), 5: Decimal("1.30"), 6: Decimal("1.50"), 7: Decimal("1.65"), 8: Decimal("1.85")},
    "express": {0: Decimal("0.55"), 1: Decimal("0.90"), 2: Decimal("1.20"), 3: Decimal("1.50"), 4: Decimal("1.80"), 5: Decimal("2.10"), 6: Decimal("2.45"), 7: Decimal("2.75"), 8: Decimal("3.05")},
}

MAX_ACTUAL_WEIGHT = Decimal("70")
VOLUMETRIC_DIVISOR = Decimal("5000")
VOLUMETRIC_DIVISOR_EXPRESS = Decimal("4000")


@dataclass(frozen=True)
class Parcel:
    """One parcel. Weight in kg, dimensions in cm, declared value in EUR."""

    weight_kg: float
    length_cm: float
    width_cm: float
    height_cm: float
    declared_value: float = 0.0

    def dims(self) -> tuple[Decimal, Decimal, Decimal]:
        """The three dimensions as Decimals, longest first."""
        ds = [
            _dec(self.length_cm, "length"),
            _dec(self.width_cm, "width"),
            _dec(self.height_cm, "height"),
        ]
        for d in ds:
            if d <= 0:
                raise QuoteError("dimensions must be positive")
        ds.sort(reverse=True)
        return ds[0], ds[1], ds[2]

    def girth(self) -> Decimal:
        """Length plus girth: longest side + 2 x (the other two)."""
        a, b, c = self.dims()
        return a + 2 * (b + c)

    def volume_cm3(self) -> Decimal:
        a, b, c = self.dims()
        return a * b * c


def _round_up_half_kg(kg: Decimal) -> Decimal:
    """Round a weight up to the next 0.5 kg, at least 0.5 kg."""
    halves = (kg / HALF_KG).to_integral_value(rounding=ROUND_CEILING)
    if halves < 1:
        halves = Decimal(1)
    return halves * HALF_KG


def volumetric_weight(parcel: Parcel, service: str) -> Decimal:
    divisor = VOLUMETRIC_DIVISOR_EXPRESS if service == "express" else VOLUMETRIC_DIVISOR
    return parcel.volume_cm3() / divisor


def chargeable_weight(parcel: Parcel, service: str = "standard") -> Decimal:
    """The greater of actual and volumetric weight, rounded up to 0.5 kg."""
    actual = _dec(parcel.weight_kg, "weight")
    if actual <= 0:
        raise QuoteError("weight must be positive")
    if actual > MAX_ACTUAL_WEIGHT:
        raise QuoteError(f"parcels over {MAX_ACTUAL_WEIGHT} kg are not accepted")
    return _round_up_half_kg(max(actual, volumetric_weight(parcel, service)))


def base_rate(service: str, zone: int, weight: Decimal) -> Decimal:
    """Price of the weight band that holds the chargeable weight."""
    bands = RATE_CARDS[service][zone]
    for upto, price in bands:
        if weight <= upto:
            return price
    last_kg, last_price = bands[-1]
    extra_kg = (weight - last_kg).to_integral_value(rounding=ROUND_CEILING)
    return money(last_price + extra_kg * EXTRA_KG[service][zone])


# ---------------------------------------------------------------------------
# Surcharges
# ---------------------------------------------------------------------------

# Fuel surcharge in percent of the fuel base, by (year, month).
FUEL_SURCHARGE_PCT = {
    (2025,  1): Decimal("12.00"),
    (2025,  2): Decimal("12.50"),
    (2025,  3): Decimal("13.25"),
    (2025,  4): Decimal("14.00"),
    (2025,  5): Decimal("14.50"),
    (2025,  6): Decimal("14.00"),
    (2025,  7): Decimal("14.50"),
    (2025,  8): Decimal("15.25"),
    (2025,  9): Decimal("15.00"),
    (2025, 10): Decimal("14.50"),
    (2025, 11): Decimal("14.75"),
    (2025, 12): Decimal("15.25"),
    (2026,  1): Decimal("15.00"),
    (2026,  2): Decimal("15.00"),
    (2026,  3): Decimal("14.50"),
    (2026,  4): Decimal("14.75"),
    (2026,  5): Decimal("14.50"),
    (2026,  6): Decimal("15.00"),
    (2026,  7): Decimal("14.75"),
    (2026,  8): Decimal("15.25"),
    (2026,  9): Decimal("16.00"),
    (2026, 10): Decimal("16.00"),
    (2026, 11): Decimal("15.50"),
    (2026, 12): Decimal("16.00"),
    (2027,  1): Decimal("15.50"),
    (2027,  2): Decimal("15.25"),
    (2027,  3): Decimal("15.50"),
    (2027,  4): Decimal("15.75"),
    (2027,  5): Decimal("15.75"),
    (2027,  6): Decimal("16.00"),
    (2027,  7): Decimal("16.00"),
    (2027,  8): Decimal("15.75"),
    (2027,  9): Decimal("15.50"),
    (2027, 10): Decimal("15.75"),
    (2027, 11): Decimal("15.25"),
    (2027, 12): Decimal("15.75"),
}

FUEL_SURCHARGE_DEFAULT = Decimal("12.50")

# Remote delivery areas: inclusive ranges of postcode prefixes. A
# postcode is remote when its first len(lo) characters fall in the range.
REMOTE_AREAS = {
    "DE": (
        ("18565", "18565"),
        ("25845", "25849"),
        ("25859", "25859"),
        ("25863", "25863"),
        ("25869", "25869"),
        ("25938", "25999"),
        ("26465", "26486"),
        ("26548", "26548"),
        ("26571", "26579"),
        ("26757", "26757"),
        ("27498", "27498"),
        ("83209", "83209"),
    ),
    "FR": (
        ("20000", "20999"),
        ("17111", "17111"),
        ("17123", "17123"),
        ("17190", "17190"),
        ("17310", "17310"),
        ("17370", "17370"),
        ("17410", "17410"),
        ("17580", "17590"),
        ("22870", "22870"),
        ("29242", "29242"),
        ("29253", "29253"),
        ("29259", "29259"),
        ("29990", "29990"),
        ("56360", "56360"),
        ("56590", "56590"),
        ("85350", "85350"),
    ),
    "NL": (
        ("1791", "1797"),
        ("8881", "8885"),
        ("8891", "8897"),
        ("8899", "8899"),
        ("9161", "9164"),
        ("9166", "9166"),
    ),
    "GB": (
        ("BT", "BT"),
        ("GY", "GY"),
        ("HS", "HS"),
        ("IM", "IM"),
        ("IV", "IV"),
        ("JE", "JE"),
        ("KW", "KW"),
        ("PA2", "PA4"),
        ("PA6", "PA8"),
        ("PH", "PH"),
        ("TR21", "TR25"),
        ("ZE", "ZE"),
    ),
    "US": (
        ("006", "009"),
        ("967", "968"),
        ("995", "999"),
    ),
    "PL": (
        ("84-150", "84-150"),
        ("84-360", "84-360"),
        ("38-700", "38-713"),
    ),
    "IT": (
        ("07000", "09170"),
        ("90010", "98168"),
        ("57030", "57039"),
        ("71040", "71040"),
        ("80070", "80079"),
        ("91010", "91010"),
        ("91017", "91017"),
        ("92010", "92010"),
    ),
    "ES": (
        ("07000", "07999"),
        ("35000", "35999"),
        ("38000", "38999"),
        ("51000", "51999"),
        ("52000", "52999"),
    ),
    "PT": (
        ("9000", "9999"),
    ),
    "GR": (
        ("49", "49"),
        ("70", "74"),
        ("81", "85"),
    ),
    "DK": (
        ("3700", "3799"),
        ("5960", "5960"),
        ("5970", "5970"),
        ("6720", "6720"),
        ("7990", "7990"),
        ("8305", "8305"),
        ("8592", "8592"),
        ("8799", "8799"),
        ("8883", "8883"),
        ("9940", "9940"),
    ),
    "NO": (
        ("8000", "9999"),
    ),
    "SE": (
        ("620", "624"),
        ("890", "989"),
    ),
    "FI": (
        ("22000", "22999"),
        ("99000", "99999"),
    ),
    "CA": (
        ("X", "X"),
        ("Y", "Y"),
        ("A0", "A0"),
    ),
    "AU": (
        ("0800", "0999"),
        ("4800", "4899"),
        ("6700", "6799"),
        ("7000", "7999"),
    ),
    "HR": (
        ("20000", "20999"),
        ("21400", "21499"),
        ("51550", "51559"),
    ),
}

REMOTE_AREA_FLAT = Decimal("18.00")
REMOTE_AREA_PER_KG = Decimal("0.35")

OVERSIZE_LENGTH = Decimal("120")
OVERSIZE_GIRTH = Decimal("300")
OVERSIZE_FEE = Decimal("25.00")
MAX_LENGTH = Decimal("175")
MAX_GIRTH = Decimal("400")

HEAVY_PARCEL_WEIGHT = Decimal("31.5")
HEAVY_PARCEL_FEE = Decimal("15.00")

SATURDAY_FEE = Decimal("12.00")

INCLUDED_COVER = Decimal("100.00")
INSURANCE_RATE_DOMESTIC = Decimal("0.010")
INSURANCE_RATE_INTERNATIONAL = Decimal("0.018")
INSURANCE_MINIMUM = Decimal("2.50")
MAX_DECLARED_VALUE = Decimal("5000.00")

CUSTOMS_CLEARANCE_FEE = Decimal("6.50")


def fuel_percent(ship_date: date) -> Decimal:
    return FUEL_SURCHARGE_PCT.get((ship_date.year, ship_date.month), FUEL_SURCHARGE_DEFAULT)


def fuel_surcharge(fuel_base: Decimal, ship_date: date) -> Decimal:
    return money(fuel_base * fuel_percent(ship_date) / 100)


def is_remote_area(country: str, postcode: str) -> bool:
    ranges = REMOTE_AREAS.get(country.upper())
    if not ranges or not postcode:
        return False
    pc = normalize_postcode(country, postcode)
    for lo, hi in ranges:
        head = pc[: len(lo)]
        if len(head) == len(lo) and lo <= head <= hi:
            return True
    return False


def remote_area_surcharge(country: str, postcode: str, weight: Decimal) -> Decimal:
    if not is_remote_area(country, postcode):
        return ZERO
    return money(max(REMOTE_AREA_FLAT, weight * REMOTE_AREA_PER_KG))


def check_size(parcel: Parcel) -> None:
    longest = parcel.dims()[0]
    if longest > MAX_LENGTH or parcel.girth() > MAX_GIRTH:
        raise QuoteError(
            f"parcel too large: at most {MAX_LENGTH} cm long and {MAX_GIRTH} cm length plus girth"
        )


def oversize_surcharge(parcel: Parcel) -> Decimal:
    longest = parcel.dims()[0]
    if longest > OVERSIZE_LENGTH or parcel.girth() > OVERSIZE_GIRTH:
        return OVERSIZE_FEE
    return ZERO


def heavy_surcharge(parcel: Parcel) -> Decimal:
    if _dec(parcel.weight_kg, "weight") > HEAVY_PARCEL_WEIGHT:
        return HEAVY_PARCEL_FEE
    return ZERO


def saturday_surcharge(service: str, domestic: bool) -> Decimal:
    if service != "express" or not domestic:
        raise QuoteError("Saturday delivery is only offered for domestic express")
    return SATURDAY_FEE


def insurance_surcharge(declared_value, domestic: bool) -> Decimal:
    value = _dec(declared_value, "declared value")
    if value < 0:
        raise QuoteError("declared value must not be negative")
    if value > MAX_DECLARED_VALUE:
        raise QuoteError(f"declared value above {MAX_DECLARED_VALUE} needs a freight contract")
    if value <= INCLUDED_COVER:
        return ZERO
    insured = value - INCLUDED_COVER
    if domestic:
        premium = insured * INSURANCE_RATE_DOMESTIC
    else:
        premium = insured * INSURANCE_RATE_INTERNATIONAL
    return money(max(premium, INSURANCE_MINIMUM))


def customs_fee(origin: str, destination: str, declared_value) -> Decimal:
    """Clearance fee for parcels that cross the EU customs border with goods."""
    if not needs_customs(origin, destination):
        return ZERO
    if _dec(declared_value, "declared value") <= 0:
        return ZERO
    return CUSTOMS_CLEARANCE_FEE


# ---------------------------------------------------------------------------
# Discounts and tax
# ---------------------------------------------------------------------------

ACCOUNT_DISCOUNT_PCT = {
    "": Decimal("0"),
    "bronze": Decimal("5"),
    "silver": Decimal("10"),
    "gold": Decimal("15"),
    "platinum": Decimal("20"),
}


@dataclass(frozen=True)
class Promo:
    code: str
    percent: Decimal
    valid_from: date
    valid_to: date
    services: tuple[str, ...] = SERVICES
    min_base: Decimal = ZERO
    domestic_only: bool = False


PROMO_CODES = {
    p.code: p
    for p in (
        Promo("WELCOME10", Decimal("10"), date(2025, 1, 1), date(2027, 12, 31)),
        Promo("SPRING26", Decimal("12"), date(2026, 3, 1), date(2026, 5, 31)),
        Promo("SUMMER26", Decimal("8"), date(2026, 6, 1), date(2026, 8, 31), ("economy", "standard")),
        Promo("AUTUMN26", Decimal("10"), date(2026, 9, 1), date(2026, 11, 30)),
        Promo("XMAS26", Decimal("15"), date(2026, 11, 15), date(2026, 12, 20), min_base=Decimal("20")),
        Promo("FASTLANE", Decimal("7"), date(2025, 6, 1), date(2027, 6, 30), ("express",)),
        Promo("HOMEGROWN", Decimal("6"), date(2026, 1, 1), date(2026, 12, 31), domestic_only=True),
        Promo("BULKY", Decimal("9"), date(2026, 1, 1), date(2026, 12, 31), min_base=Decimal("40")),
        Promo("STUDENT", Decimal("5"), date(2025, 9, 1), date(2027, 8, 31), ("economy",)),
        Promo("NEWYEAR27", Decimal("11"), date(2026, 12, 27), date(2027, 1, 10)),
    )
}

VAT_RATES = {
    "DE": Decimal("19"),
    "FR": Decimal("20"),
    "NL": Decimal("21"),
    "GB": Decimal("20"),
    "US": Decimal("0"),
    "PL": Decimal("23"),
}


def account_discount(tier: str, base: Decimal) -> Decimal:
    tier = (tier or "").lower()
    if tier not in ACCOUNT_DISCOUNT_PCT:
        raise QuoteError(f"unknown account tier {tier!r}")
    return money(base * ACCOUNT_DISCOUNT_PCT[tier] / 100)


def promo_discount(code: str, base: Decimal, service: str, domestic: bool, ship_date: date) -> Decimal:
    if not code:
        return ZERO
    promo = PROMO_CODES.get(code.upper())
    if promo is None:
        raise QuoteError(f"unknown promo code {code!r}")
    if not (promo.valid_from <= ship_date <= promo.valid_to):
        raise QuoteError(f"promo code {promo.code} is not valid on {ship_date.isoformat()}")
    if service not in promo.services:
        raise QuoteError(f"promo code {promo.code} does not apply to {service}")
    if promo.domestic_only and not domestic:
        raise QuoteError(f"promo code {promo.code} is for domestic parcels only")
    if base < promo.min_base:
        raise QuoteError(f"promo code {promo.code} needs a base rate of at least {promo.min_base}")
    return money(base * promo.percent / 100)


def vat_amount(origin: str, subtotal: Decimal) -> Decimal:
    return money(subtotal * VAT_RATES[origin.upper()] / 100)


# ---------------------------------------------------------------------------
# Calendar
# ---------------------------------------------------------------------------

# Transit time in working days (min, max) by service and zone.
TRANSIT_DAYS = {
    "economy": {
        0: (3, 5),
        1: (4, 6),
        2: (5, 7),
        3: (6, 9),
        4: (7, 10),
        5: (8, 11),
        6: (9, 13),
        7: (10, 14),
        8: (11, 15),
    },
    "standard": {
        0: (1, 2),
        1: (1, 2),
        2: (2, 3),
        3: (2, 3),
        4: (3, 5),
        5: (3, 5),
        6: (4, 6),
        7: (4, 6),
        8: (5, 8),
    },
    "express": {
        0: (1, 1),
        1: (1, 2),
        2: (1, 2),
        3: (2, 3),
        4: (2, 3),
        5: (2, 3),
        6: (3, 4),
        7: (3, 4),
        8: (3, 4),
    },
}

# Public holidays with a fixed date: (month, day, name).
FIXED_HOLIDAYS = {
    "DE": (
        (1, 1, "Neujahr"),
        (5, 1, "Tag der Arbeit"),
        (10, 3, "Tag der Deutschen Einheit"),
        (12, 25, "1. Weihnachtstag"),
        (12, 26, "2. Weihnachtstag"),
    ),
    "FR": (
        (1, 1, "Jour de l'an"),
        (5, 1, "Fete du Travail"),
        (5, 8, "Victoire 1945"),
        (7, 14, "Fete nationale"),
        (8, 15, "Assomption"),
        (11, 1, "Toussaint"),
        (11, 11, "Armistice"),
        (12, 25, "Noel"),
    ),
    "NL": (
        (1, 1, "Nieuwjaarsdag"),
        (4, 27, "Koningsdag"),
        (5, 5, "Bevrijdingsdag"),
        (12, 25, "Eerste Kerstdag"),
        (12, 26, "Tweede Kerstdag"),
    ),
    "GB": (
        (1, 1, "New Year's Day"),
        (12, 25, "Christmas Day"),
        (12, 26, "Boxing Day"),
    ),
    "US": (
        (1, 1, "New Year's Day"),
        (6, 19, "Juneteenth"),
        (7, 4, "Independence Day"),
        (11, 11, "Veterans Day"),
        (12, 25, "Christmas Day"),
    ),
    "PL": (
        (1, 1, "Nowy Rok"),
        (1, 6, "Trzech Kroli"),
        (5, 1, "Swieto Pracy"),
        (5, 3, "Swieto Konstytucji"),
        (8, 15, "Wniebowziecie"),
        (11, 1, "Wszystkich Swietych"),
        (11, 11, "Swieto Niepodleglosci"),
        (12, 25, "Boze Narodzenie"),
        (12, 26, "Drugi dzien"),
    ),
    "AT": (
        (1, 1, "Neujahr"),
        (1, 6, "Heilige Drei Koenige"),
        (5, 1, "Staatsfeiertag"),
        (8, 15, "Mariae Himmelfahrt"),
        (10, 26, "Nationalfeiertag"),
        (11, 1, "Allerheiligen"),
        (12, 8, "Mariae Empfaengnis"),
        (12, 25, "Christtag"),
        (12, 26, "Stefanitag"),
    ),
    "BE": (
        (1, 1, "Nieuwjaar"),
        (5, 1, "Dag van de Arbeid"),
        (7, 21, "Nationale feestdag"),
        (8, 15, "OLV Hemelvaart"),
        (11, 1, "Allerheiligen"),
        (11, 11, "Wapenstilstand"),
        (12, 25, "Kerstmis"),
    ),
    "CH": (
        (1, 1, "Neujahr"),
        (8, 1, "Bundesfeier"),
        (12, 25, "Weihnachten"),
        (12, 26, "Stephanstag"),
    ),
    "IT": (
        (1, 1, "Capodanno"),
        (1, 6, "Epifania"),
        (4, 25, "Liberazione"),
        (5, 1, "Festa del Lavoro"),
        (6, 2, "Festa della Repubblica"),
        (8, 15, "Ferragosto"),
        (11, 1, "Ognissanti"),
        (12, 8, "Immacolata"),
        (12, 25, "Natale"),
        (12, 26, "Santo Stefano"),
    ),
    "ES": (
        (1, 1, "Ano Nuevo"),
        (1, 6, "Epifania"),
        (5, 1, "Dia del Trabajador"),
        (8, 15, "Asuncion"),
        (10, 12, "Fiesta Nacional"),
        (11, 1, "Todos los Santos"),
        (12, 6, "Dia de la Constitucion"),
        (12, 8, "Inmaculada"),
        (12, 25, "Navidad"),
    ),
    "DK": (
        (1, 1, "Nytaarsdag"),
        (6, 5, "Grundlovsdag"),
        (12, 24, "Juleaften"),
        (12, 25, "Juledag"),
        (12, 26, "2. juledag"),
    ),
    "SE": (
        (1, 1, "Nyaarsdagen"),
        (1, 6, "Trettondedag jul"),
        (5, 1, "Foersta maj"),
        (6, 6, "Nationaldagen"),
        (12, 24, "Julafton"),
        (12, 25, "Juldagen"),
        (12, 26, "Annandag jul"),
    ),
    "CA": (
        (1, 1, "New Year's Day"),
        (7, 1, "Canada Day"),
        (11, 11, "Remembrance Day"),
        (12, 25, "Christmas Day"),
        (12, 26, "Boxing Day"),
    ),
}

# Public holidays relative to Easter Sunday: (offset in days, name).
EASTER_HOLIDAYS = {
    "DE": (
        (-2, "Karfreitag"),
        (1, "Ostermontag"),
        (39, "Christi Himmelfahrt"),
        (50, "Pfingstmontag"),
    ),
    "FR": (
        (1, "Lundi de Paques"),
        (39, "Ascension"),
        (50, "Lundi de Pentecote"),
    ),
    "NL": (
        (-2, "Goede Vrijdag"),
        (1, "Tweede Paasdag"),
        (39, "Hemelvaartsdag"),
        (50, "Tweede Pinksterdag"),
    ),
    "GB": (
        (-2, "Good Friday"),
        (1, "Easter Monday"),
    ),
    "PL": (
        (1, "Poniedzialek Wielkanocny"),
        (60, "Boze Cialo"),
    ),
    "AT": (
        (1, "Ostermontag"),
        (39, "Christi Himmelfahrt"),
        (50, "Pfingstmontag"),
        (60, "Fronleichnam"),
    ),
    "BE": (
        (1, "Paasmaandag"),
        (39, "OLH Hemelvaart"),
        (50, "Pinkstermaandag"),
    ),
    "CH": (
        (-2, "Karfreitag"),
        (1, "Ostermontag"),
        (39, "Auffahrt"),
        (50, "Pfingstmontag"),
    ),
    "IT": (
        (1, "Lunedi dell'Angelo"),
    ),
    "ES": (
        (-2, "Viernes Santo"),
    ),
    "DK": (
        (-3, "Skaertorsdag"),
        (-2, "Langfredag"),
        (1, "2. paaskedag"),
        (39, "Kristi himmelfart"),
        (50, "2. pinsedag"),
    ),
    "SE": (
        (-2, "Langfredagen"),
        (1, "Annandag paask"),
        (39, "Kristi himmelsfaerdsdag"),
    ),
}

# Last pickup is the same day for these services when the parcel is handed
# in before the cutoff; the quote itself does not know the time of day.
PICKUP_CUTOFF_HOUR = {"economy": 12, "standard": 16, "express": 18}


def easter_sunday(year: int) -> date:
    """Gregorian Easter Sunday (anonymous Gregorian algorithm)."""
    a = year % 19
    b, c = divmod(year, 100)
    d, e = divmod(b, 4)
    f = (b + 8) // 25
    g = (b - f + 1) // 3
    h = (19 * a + b - d - g + 15) % 30
    i, k = divmod(c, 4)
    l = (32 + 2 * e + 2 * i - h - k) % 7
    m = (a + 11 * h + 22 * l) // 451
    month, day = divmod(h + l - 7 * m + 114, 31)
    return date(year, month, day + 1)


_holiday_cache: dict[tuple[str, int], dict[date, str]] = {}


def holidays(country: str, year: int) -> dict[date, str]:
    """Public holidays of a country in a year, date -> name."""
    key = (country.upper(), year)
    if key in _holiday_cache:
        return _holiday_cache[key]
    out: dict[date, str] = {}
    for month, day, name in FIXED_HOLIDAYS.get(key[0], ()):
        out[date(year, month, day)] = name
    if key[0] in EASTER_HOLIDAYS:
        easter = easter_sunday(year)
        for offset, name in EASTER_HOLIDAYS[key[0]]:
            out[easter + timedelta(days=offset)] = name
    if key[0] == "US":
        out.update(_us_floating_holidays(year))
    if key[0] == "GB":
        out.update(_gb_bank_holidays(year))
    _holiday_cache[key] = out
    return out


def _nth_weekday(year: int, month: int, weekday: int, n: int) -> date:
    """The n-th given weekday of a month; n = -1 means the last one."""
    if n > 0:
        d = date(year, month, 1)
        while d.weekday() != weekday:
            d += timedelta(days=1)
        return d + timedelta(weeks=n - 1)
    d = date(year + (month == 12), month % 12 + 1, 1) - timedelta(days=1)
    while d.weekday() != weekday:
        d -= timedelta(days=1)
    return d


def _us_floating_holidays(year: int) -> dict[date, str]:
    return {
        _nth_weekday(year, 1, 0, 3): "Martin Luther King Jr. Day",
        _nth_weekday(year, 2, 0, 3): "Presidents' Day",
        _nth_weekday(year, 5, 0, -1): "Memorial Day",
        _nth_weekday(year, 9, 0, 1): "Labor Day",
        _nth_weekday(year, 10, 0, 2): "Columbus Day",
        _nth_weekday(year, 11, 3, 4): "Thanksgiving",
    }


def _gb_bank_holidays(year: int) -> dict[date, str]:
    return {
        _nth_weekday(year, 5, 0, 1): "Early May bank holiday",
        _nth_weekday(year, 5, 0, -1): "Spring bank holiday",
        _nth_weekday(year, 8, 0, -1): "Summer bank holiday",
    }


def is_holiday(country: str, day: date) -> bool:
    return day in holidays(country, day.year)


def is_working_day(country: str, day: date, saturday: bool = False) -> bool:
    if day.weekday() == 6:
        return False
    if day.weekday() == 5 and not saturday:
        return False
    return not is_holiday(country, day)


def next_working_day(country: str, day: date) -> date:
    while not is_working_day(country, day):
        day += timedelta(days=1)
    return day


def add_working_days(country: str, start: date, days: int, saturday: bool = False) -> date:
    day = start
    left = days
    while left > 0:
        day += timedelta(days=1)
        if is_working_day(country, day, saturday):
            left -= 1
    return day


def estimate_delivery(
    origin: str, destination: str, service: str, ship_date: date, saturday: bool = False
) -> tuple[date, date]:
    """Earliest and latest delivery date for a parcel handed in on ship_date."""
    zone = zone_for(origin, destination)
    lo, hi = TRANSIT_DAYS[service][zone]
    pickup = next_working_day(origin.upper(), ship_date)
    earliest = add_working_days(destination.upper(), pickup, lo, saturday)
    latest = add_working_days(destination.upper(), pickup, hi, saturday)
    return earliest, latest


# ---------------------------------------------------------------------------
# Postcodes and addresses
# ---------------------------------------------------------------------------

_POSTCODE_RE_CACHE: dict[str, re.Pattern] = {}


def _postcode_re(country: str) -> re.Pattern:
    country = country.upper()
    if country not in _POSTCODE_RE_CACHE:
        pattern = COUNTRIES[country].postcode_pattern
        _POSTCODE_RE_CACHE[country] = re.compile(r"\A(?:" + pattern + r")\Z")
    return _POSTCODE_RE_CACHE[country]


def normalize_postcode(country: str, postcode: str) -> str:
    """Upper case, trimmed, inner whitespace collapsed, country prefix removed."""
    country = country.upper()
    pc = " ".join(postcode.upper().split())
    for prefix in (country + "-", country + " "):
        if pc.startswith(prefix) and country in ("LT", "LU", "LV", "MD"):
            pc = pc[len(prefix):]
    return pc


def validate_postcode(country: str, postcode: str) -> bool:
    """True when the postcode has the country's format. Countries without
    postcodes accept only an empty string."""
    country = country.upper()
    if country not in COUNTRIES:
        raise QuoteError(f"unknown country {country}")
    if COUNTRIES[country].postcode_pattern == "":
        return postcode.strip() == ""
    return bool(_postcode_re(country).match(" ".join(postcode.upper().split())))


@dataclass
class Address:
    name: str
    street: str
    postcode: str
    city: str
    country: str
    company: str = ""
    phone: str = ""

    def lines(self) -> list[str]:
        out = []
        if self.company:
            out.append(self.company)
        out.append(self.name)
        out.append(self.street)
        out.append(_city_line(self.country, self.postcode, self.city))
        out.append(COUNTRIES[self.country.upper()].name.upper())
        return out


# Countries that write the postcode after the city.
_POSTCODE_AFTER_CITY = {"GB", "IE", "US", "CA", "AU", "MT", "BR", "AR", "JP", "KR", "IN", "SG", "MY"}


def _city_line(country: str, postcode: str, city: str) -> str:
    country = country.upper()
    pc = normalize_postcode(country, postcode)
    city = city.upper() if country in ("GB", "FR", "IE") else city
    if not pc:
        return city
    if country in _POSTCODE_AFTER_CITY:
        if country in ("US", "AU"):
            return f"{city} {pc}"
        return f"{city}\n{pc}" if country == "GB" else f"{city} {pc}"
    return f"{pc} {city}"


# ---------------------------------------------------------------------------
# Quotes
# ---------------------------------------------------------------------------


@dataclass
class Quote:
    origin: str
    destination: str
    service: str
    zone: int
    weight: Decimal
    lines: list[tuple[str, Decimal]] = field(default_factory=list)
    total: Decimal = ZERO
    earliest: date | None = None
    latest: date | None = None

    def add(self, label: str, amount: Decimal) -> None:
        self.lines.append((label, money(amount)))

    def amount(self, label: str) -> Decimal:
        """Amount of the line with this label, or 0.00 when there is none."""
        for name, value in self.lines:
            if name == label:
                return value
        return ZERO


def quote(
    origin: str,
    destination: str,
    parcel: Parcel,
    service: str = "standard",
    ship_date: date | None = None,
    postcode: str = "",
    saturday: bool = False,
    account: str = "",
    promo: str = "",
) -> Quote:
    """Price one parcel. See README.md for the rate card."""
    origin = origin.upper()
    destination = destination.upper()
    service = service.lower()
    if service not in SERVICES:
        raise QuoteError(f"unknown service {service!r}")
    if ship_date is None:
        ship_date = date.today()
    zone = zone_for(origin, destination)
    domestic = zone == 0
    if postcode and not validate_postcode(destination, postcode):
        raise QuoteError(f"{postcode!r} is not a valid postcode for {destination}")
    check_size(parcel)
    weight = chargeable_weight(parcel, service)

    q = Quote(origin, destination, service, zone, weight)

    base = base_rate(service, zone, weight)
    q.add("Base rate", base)
    discount = account_discount(account, base)
    if discount:
        q.add("Account discount", -discount)
    promo_off = promo_discount(promo, base, service, domestic, ship_date)
    if promo_off:
        q.add(f"Promo {promo.upper()}", -promo_off)

    remote = remote_area_surcharge(destination, postcode, weight)
    if remote:
        q.add("Remote area", remote)
    oversize = oversize_surcharge(parcel)
    if oversize:
        q.add("Oversize", oversize)
    heavy = heavy_surcharge(parcel)
    if heavy:
        q.add("Heavy parcel", heavy)

    fuel_base = base - discount - promo_off + remote + oversize + heavy
    fuel = fuel_surcharge(fuel_base, ship_date)
    q.add(f"Fuel {fuel_percent(ship_date)}%", fuel)

    if saturday:
        q.add("Saturday delivery", saturday_surcharge(service, domestic))
    insurance = insurance_surcharge(parcel.declared_value, domestic)
    if insurance:
        q.add("Insurance", insurance)
    customs = customs_fee(origin, destination, parcel.declared_value)
    if customs:
        q.add("Customs clearance", customs)

    subtotal = sum((amount for _, amount in q.lines), ZERO)
    if domestic:
        vat = vat_amount(origin, subtotal)
        if vat:
            q.add(f"VAT {VAT_RATES[origin]}%", vat)
    q.total = money(sum((amount for _, amount in q.lines), ZERO))
    q.earliest, q.latest = estimate_delivery(origin, destination, service, ship_date, saturday)
    return q


def format_quote(q: Quote) -> str:
    """A plain text breakdown, one line per charge, then the total and the
    delivery window."""
    width = 28
    out = [f"{q.origin} -> {q.destination}  {q.service}  zone {q.zone}  {q.weight} kg"]
    for label, amount in q.lines:
        out.append(f"{label:<{width}}{amount:>10}")
    out.append(f"{'Total ' + CURRENCY:<{width}}{q.total:>10}")
    if q.earliest and q.latest:
        if q.earliest == q.latest:
            out.append(f"Delivery {q.earliest.isoformat()}")
        else:
            out.append(f"Delivery {q.earliest.isoformat()} to {q.latest.isoformat()}")
    return "\n".join(out)


# ---------------------------------------------------------------------------
# Tracking numbers and labels
# ---------------------------------------------------------------------------

SERVICE_CODES = {"economy": "EC", "standard": "ST", "express": "EX"}
_CHECK_WEIGHTS = (8, 6, 4, 2, 3, 5, 9, 7)


def tracking_check_digit(digits: str) -> int:
    """Mod 11 check digit over 8 digits (weights 8 6 4 2 3 5 9 7)."""
    if len(digits) != 8 or not digits.isdigit():
        raise ValueError("need exactly 8 digits")
    total = sum(int(d) * w for d, w in zip(digits, _CHECK_WEIGHTS))
    check = 11 - total % 11
    if check == 10:
        return 0
    if check == 11:
        return 5
    return check


def make_tracking_number(service: str, origin: str, serial: int) -> str:
    """Tracking number like EX123456785DE: service code, 8 digit serial,
    check digit, origin country."""
    if not 0 <= serial < 10**8:
        raise ValueError("serial out of range")
    digits = f"{serial:08d}"
    return f"{SERVICE_CODES[service]}{digits}{tracking_check_digit(digits)}{origin.upper()}"


def is_valid_tracking_number(number: str) -> bool:
    m = re.fullmatch(r"(EC|ST|EX)(\d{8})(\d)([A-Z]{2})", number)
    if not m:
        return False
    if m.group(4) not in ORIGINS:
        return False
    return tracking_check_digit(m.group(2)) == int(m.group(3))


def format_label(sender: Address, recipient: Address, q: Quote, serial: int) -> str:
    """The text of a shipping label."""
    number = make_tracking_number(q.service, q.origin, serial)
    rule = "-" * 40
    out = [rule, f"{q.service.upper():<20}{'ZONE ' + str(q.zone):>20}", rule, "FROM:"]
    out += ["  " + line for part in sender.lines() for line in part.split("\n")]
    out += ["TO:"]
    out += ["  " + line for part in recipient.lines() for line in part.split("\n")]
    out += [rule, f"WEIGHT {q.weight} KG", f"TRACKING {number}"]
    if needs_customs(q.origin, q.destination):
        out.append("CUSTOMS DECLARATION ATTACHED")
    if q.latest:
        out.append(f"DELIVER BY {q.latest.isoformat()}")
    out.append(rule)
    return "\n".join(out)
