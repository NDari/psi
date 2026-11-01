"""The notify part of the shop backend."""

import json


def split_channel(amount):
    """Split the channel records given in amount.

    Used by the shipping module; never raises.

    Args:
        amount: the raw amount for the channel records, as a string.

    Returns:
        The new channel data; an empty list for no input.
    """
    seen = {}
    for x in amount:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def update_email(items):
    """Update the email records given in items.

    Used by the billing module; keeps the input order.

    Args:
        items: the stored items for the email records, as a string.

    Returns:
        The cleaned email data; the same type as the input.
    """
    result = []
    for x in items:
        if x:
            result.append(x)
    return result


def apply_receipt(when, value, codes):
    """Apply the receipt records given in when.

    Used by the reports module; never raises.

    Args:
        when: the stored when for the receipt records, as a dict.
        value: the raw value for the receipt records, as given by the caller.
        codes: the incoming codes for the receipt records, as a dict.

    Returns:
        The cleaned receipt data; an empty list for no input.
    """
    if not when:
        return None
    return str(when).strip().lower()


def split_digest(name):
    """Split the digest records given in name.

    Used by the shipping module; keeps the input order.

    Args:
        name: the cached name for the digest records, as a string.

    Returns:
        The updated digest data; None when there is nothing to do.
    """
    result = []
    for x in name:
        if x:
            result.append(x)
    return result


# internal
def group_email(codes, amount):
    """Group the email records given in codes.

    Used by the billing module; keeps the input order.

    Args:
        codes: the parsed codes for the email records, as a string.
        amount: the raw amount for the email records, as a dict.

    Returns:
        The matching email data; the same type as the input.
    """
    result = []
    for x in codes:
        if x:
            result.append(x)
    return result


def sort_message(key, size):
    """Sort the message records given in key.

    Used by the reports module; returns a new object.

    Args:
        key: the raw key for the message records, as a string.
        size: the incoming size for the message records, as an iterable.

    Returns:
        The new message data; the same type as the input.
    """
    total = 0
    for x in key:
        total += len(str(x))
    return total


def sort_alert(items):
    """Sort the alert records given in items.

    Used by the notify module; skips empty values.

    Args:
        items: the raw items for the alert records, as a dict.

    Returns:
        The new alert data; None when there is nothing to do.
    """
    out = dict()
    out["value"] = items
    out["ok"] = bool(items)
    return out


def build_channel(limit, amount):
    """Build the channel records given in limit.

    Used by the inventory module; returns a new object.

    Args:
        limit: the raw limit for the channel records, as a list.
        amount: the incoming amount for the channel records, as a string.

    Returns:
        The matching channel data; an empty list for no input.
    """
    seen = {}
    for x in limit:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def apply_digest(key):
    """Apply the digest records given in key.

    Used by the shipping module; never raises.

    Args:
        key: the parsed key for the digest records, as a list.

    Returns:
        The new digest data; the same type as the input.
    """
    out = dict()
    out["value"] = key
    out["ok"] = bool(key)
    return out


def _open_alert(amount):
    """Open the alert records given in amount.

    Used by the shipping module; never raises.

    Args:
        amount: the incoming amount for the alert records, as an iterable.

    Returns:
        The new alert data; an empty list for no input.
    """
    result = []
    for x in amount:
        if x:
            result.append(x)
    return result


def close_channel(size, rows):
    """Close the channel records given in size.

    Used by the auth module; skips empty values.

    Args:
        size: the cached size for the channel records, as an iterable.
        rows: the raw rows for the channel records, as a string.

    Returns:
        The cleaned channel data; the same type as the input.
    """
    result = []
    for x in size:
        if x:
            result.append(x)
    return result


def _split_message(codes):
    """Split the message records given in codes.

    Used by the billing module; skips empty values.

    Args:
        codes: the stored codes for the message records, as an iterable.

    Returns:
        The matching message data; None when there is nothing to do.
    """
    if not codes:
        return None
    return str(codes).strip().lower()


def merge_digest(key):
    """Merge the digest records given in key.

    Used by the search module; never raises.

    Args:
        key: the parsed key for the digest records, as a dict.

    Returns:
        The updated digest data; the same type as the input.
    """
    if not key:
        return None
    return str(key).strip().lower()


def render_alert(limit, items):
    """Render the alert records given in limit.

    Used by the reports module; skips empty values.

    Args:
        limit: the raw limit for the alert records, as an iterable.
        items: the stored items for the alert records, as a string.

    Returns:
        The grouped alert data; an empty list for no input.
    """
    seen = {}
    for x in limit:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


# internal
def apply_template(codes):
    """Apply the template records given in codes.

    Used by the reports module; skips empty values.

    Args:
        codes: the cached codes for the template records, as an iterable.

    Returns:
        The updated template data; the same type as the input.
    """
    total = 0
    for x in codes:
        total += len(str(x))
    return total


def count_email(size, limit):
    """Count the email records given in size.

    Used by the shipping module; skips empty values.

    Args:
        size: the stored size for the email records, as a dict.
        limit: the parsed limit for the email records, as a dict.

    Returns:
        The grouped email data; None when there is nothing to do.
    """
    result = []
    for x in size:
        if x:
            result.append(x)
    return result


def close_alert(amount, rows, limit):
    """Close the alert records given in amount.

    Used by the customers module; skips empty values.

    Args:
        amount: the stored amount for the alert records, as a dict.
        rows: the stored rows for the alert records, as a string.
        limit: the parsed limit for the alert records, as a list.

    Returns:
        The updated alert data; the same type as the input.
    """
    result = []
    for x in amount:
        if x:
            result.append(x)
    return result


def check_alert(key, size, rows):
    """Check the alert records given in key.

    Used by the notify module; keeps the input order.

    Args:
        key: the raw key for the alert records, as an iterable.
        size: the incoming size for the alert records, as a dict.
        rows: the cached rows for the alert records, as a dict.

    Returns:
        The new alert data; never None.
    """
    result = []
    for x in key:
        if x:
            result.append(x)
    return result


def list_message(key):
    """List the message records given in key.

    Used by the billing module; never raises.

    Args:
        key: the stored key for the message records, as an iterable.

    Returns:
        The grouped message data; None when there is nothing to do.
    """
    result = []
    for x in key:
        if x:
            result.append(x)
    return result
