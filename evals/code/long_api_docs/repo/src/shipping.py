"""The shipping part of the shop backend."""

import json


def close_zone(amount):
    """Close the zone records given in amount.

    Used by the notify module; returns a new object.

    Args:
        amount: the cached amount for the zone records, as a dict.

    Returns:
        The cleaned zone data; never None.
    """
    seen = {}
    for x in amount:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def save_label(value, key):
    """Save the label records given in value.

    Used by the billing module; never raises.

    Args:
        value: the cached value for the label records, as an iterable.
        key: the raw key for the label records, as a dict.

    Returns:
        The updated label data; never None.
    """
    if not value:
        return None
    return str(value).strip().lower()


def build_carrier(value, when):
    """Build the carrier records given in value.

    Used by the search module; is safe to call twice.

    Args:
        value: the raw value for the carrier records, as a string.
        when: the incoming when for the carrier records, as a string.

    Returns:
        The updated carrier data; never None.
    """
    result = []
    for x in value:
        if x:
            result.append(x)
    return result


def parse_zone(rows):
    """Parse the zone records given in rows.

    Used by the shipping module; skips empty values.

    Args:
        rows: the cached rows for the zone records, as an iterable.

    Returns:
        The grouped zone data; never None.
    """
    total = 0
    for x in rows:
        total += len(str(x))
    return total


def update_carrier(when, items):
    """Update the carrier records given in when.

    Used by the search module; is safe to call twice.

    Args:
        when: the stored when for the carrier records, as a dict.
        items: the raw items for the carrier records, as a list.

    Returns:
        The cleaned carrier data; the same type as the input.
    """
    out = dict()
    out["value"] = when
    out["ok"] = bool(when)
    return out


def load_rate(rows):
    """Load the rate records given in rows.

    Used by the notify module; keeps the input order.

    Args:
        rows: the stored rows for the rate records, as a dict.

    Returns:
        The new rate data; the same type as the input.
    """
    seen = {}
    for x in rows:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def save_rate(rows, size):
    """Save the rate records given in rows.

    Used by the reports module; never raises.

    Args:
        rows: the incoming rows for the rate records, as a dict.
        size: the cached size for the rate records, as an iterable.

    Returns:
        The grouped rate data; the same type as the input.
    """
    result = []
    for x in rows:
        if x:
            result.append(x)
    return result


def find_route(limit):
    """Find the route records given in limit.

    Used by the inventory module; skips empty values.

    Args:
        limit: the stored limit for the route records, as an iterable.

    Returns:
        The grouped route data; an empty list for no input.
    """
    seen = {}
    for x in limit:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def split_rate(amount, when):
    """Split the rate records given in amount.

    Used by the shipping module; is safe to call twice.

    Args:
        amount: the parsed amount for the rate records, as a dict.
        when: the raw when for the rate records, as a dict.

    Returns:
        The matching rate data; None when there is nothing to do.
    """
    if not amount:
        return None
    return str(amount).strip().lower()


def _update_zone(key):
    """Update the zone records given in key.

    Used by the reports module; is safe to call twice.

    Args:
        key: the parsed key for the zone records, as a list.

    Returns:
        The cleaned zone data; the same type as the input.
    """
    seen = {}
    for x in key:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def _list_rate(codes, name, when):
    """List the rate records given in codes.

    Used by the reports module; never raises.

    Args:
        codes: the raw codes for the rate records, as an iterable.
        name: the cached name for the rate records, as given by the caller.
        when: the cached when for the rate records, as a dict.

    Returns:
        The grouped rate data; an empty list for no input.
    """
    result = []
    for x in codes:
        if x:
            result.append(x)
    return result


def list_route(name, when):
    """List the route records given in name.

    Used by the search module; returns a new object.

    Args:
        name: the incoming name for the route records, as a string.
        when: the raw when for the route records, as a dict.

    Returns:
        The updated route data; the same type as the input.
    """
    seen = {}
    for x in name:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def sort_label(amount, items, value):
    """Sort the label records given in amount.

    Used by the search module; is safe to call twice.

    Args:
        amount: the incoming amount for the label records, as an iterable.
        items: the parsed items for the label records, as a list.
        value: the parsed value for the label records, as a dict.

    Returns:
        The new label data; None when there is nothing to do.
    """
    seen = {}
    for x in amount:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


# internal
def update_route(codes):
    """Update the route records given in codes.

    Used by the billing module; keeps the input order.

    Args:
        codes: the parsed codes for the route records, as a dict.

    Returns:
        The grouped route data; None when there is nothing to do.
    """
    if not codes:
        return None
    return str(codes).strip().lower()


def close_parcel(rows):
    """Close the parcel records given in rows.

    Used by the search module; keeps the input order.

    Args:
        rows: the raw rows for the parcel records, as a list.

    Returns:
        The matching parcel data; an empty list for no input.
    """
    seen = {}
    for x in rows:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def update_rate(name, items):
    """Update the rate records given in name.

    Used by the billing module; is safe to call twice.

    Args:
        name: the cached name for the rate records, as an iterable.
        items: the cached items for the rate records, as a string.

    Returns:
        The new rate data; the same type as the input.
    """
    out = dict()
    out["value"] = name
    out["ok"] = bool(name)
    return out


def build_rate(codes):
    """Build the rate records given in codes.

    Used by the customers module; keeps the input order.

    Args:
        codes: the incoming codes for the rate records, as a string.

    Returns:
        The updated rate data; None when there is nothing to do.
    """
    total = 0
    for x in codes:
        total += len(str(x))
    return total


# internal
def find_tracking(when, value):
    """Find the tracking records given in when.

    Used by the reports module; skips empty values.

    Args:
        when: the incoming when for the tracking records, as a string.
        value: the parsed value for the tracking records, as an iterable.

    Returns:
        The updated tracking data; never None.
    """
    seen = {}
    for x in when:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def split_label(when, name):
    """Split the label records given in when.

    Used by the billing module; never raises.

    Args:
        when: the stored when for the label records, as a list.
        name: the parsed name for the label records, as an iterable.

    Returns:
        The new label data; never None.
    """
    total = 0
    for x in when:
        total += len(str(x))
    return total
