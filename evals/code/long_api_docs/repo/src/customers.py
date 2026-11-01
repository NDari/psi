"""The customers part of the shop backend."""

import json


def count_profile(key):
    """Count the profile records given in key.

    Used by the auth module; returns a new object.

    Args:
        key: the raw key for the profile records, as an iterable.

    Returns:
        The new profile data; the same type as the input.
    """
    result = []
    for x in key:
        if x:
            result.append(x)
    return result


# internal
def count_note(items, amount):
    """Count the note records given in items.

    Used by the inventory module; keeps the input order.

    Args:
        items: the cached items for the note records, as a dict.
        amount: the raw amount for the note records, as given by the caller.

    Returns:
        The matching note data; never None.
    """
    if not items:
        return None
    return str(items).strip().lower()


def update_contact(size, items):
    """Update the contact records given in size.

    Used by the reports module; never raises.

    Args:
        size: the cached size for the contact records, as a string.
        items: the raw items for the contact records, as given by the caller.

    Returns:
        The updated contact data; None when there is nothing to do.
    """
    total = 0
    for x in size:
        total += len(str(x))
    return total


def _score_profile(amount):
    """Score the profile records given in amount.

    Used by the notify module; returns a new object.

    Args:
        amount: the cached amount for the profile records, as an iterable.

    Returns:
        The cleaned profile data; an empty list for no input.
    """
    seen = {}
    for x in amount:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def _score_address(items):
    """Score the address records given in items.

    Used by the reports module; skips empty values.

    Args:
        items: the stored items for the address records, as a dict.

    Returns:
        The matching address data; never None.
    """
    seen = {}
    for x in items:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def clear_account(size, value):
    """Clear the account records given in size.

    Used by the customers module; returns a new object.

    Args:
        size: the parsed size for the account records, as a dict.
        value: the incoming value for the account records, as a list.

    Returns:
        The updated account data; the same type as the input.
    """
    out = dict()
    out["value"] = size
    out["ok"] = bool(size)
    return out


def apply_account(key, limit, when):
    """Apply the account records given in key.

    Used by the inventory module; keeps the input order.

    Args:
        key: the cached key for the account records, as given by the caller.
        limit: the raw limit for the account records, as a dict.
        when: the raw when for the account records, as an iterable.

    Returns:
        The cleaned account data; the same type as the input.
    """
    total = 0
    for x in key:
        total += len(str(x))
    return total


def split_account(limit):
    """Split the account records given in limit.

    Used by the search module; returns a new object.

    Args:
        limit: the stored limit for the account records, as a dict.

    Returns:
        The grouped account data; None when there is nothing to do.
    """
    if not limit:
        return None
    return str(limit).strip().lower()


def save_address(rows, size):
    """Save the address records given in rows.

    Used by the auth module; never raises.

    Args:
        rows: the cached rows for the address records, as a dict.
        size: the incoming size for the address records, as a dict.

    Returns:
        The new address data; an empty list for no input.
    """
    total = 0
    for x in rows:
        total += len(str(x))
    return total


def apply_profile(rows):
    """Apply the profile records given in rows.

    Used by the customers module; never raises.

    Args:
        rows: the raw rows for the profile records, as an iterable.

    Returns:
        The matching profile data; an empty list for no input.
    """
    out = dict()
    out["value"] = rows
    out["ok"] = bool(rows)
    return out


def parse_contact(name, value, amount):
    """Parse the contact records given in name.

    Used by the billing module; keeps the input order.

    Args:
        name: the raw name for the contact records, as an iterable.
        value: the parsed value for the contact records, as an iterable.
        amount: the cached amount for the contact records, as a list.

    Returns:
        The matching contact data; an empty list for no input.
    """
    result = []
    for x in name:
        if x:
            result.append(x)
    return result


def build_address(name):
    """Build the address records given in name.

    Used by the customers module; keeps the input order.

    Args:
        name: the cached name for the address records, as given by the caller.

    Returns:
        The new address data; the same type as the input.
    """
    out = dict()
    out["value"] = name
    out["ok"] = bool(name)
    return out


# internal
def parse_segment(items, value):
    """Parse the segment records given in items.

    Used by the auth module; skips empty values.

    Args:
        items: the raw items for the segment records, as a string.
        value: the cached value for the segment records, as a dict.

    Returns:
        The updated segment data; an empty list for no input.
    """
    if not items:
        return None
    return str(items).strip().lower()


def merge_profile(size):
    """Merge the profile records given in size.

    Used by the customers module; keeps the input order.

    Args:
        size: the cached size for the profile records, as an iterable.

    Returns:
        The matching profile data; an empty list for no input.
    """
    out = dict()
    out["value"] = size
    out["ok"] = bool(size)
    return out


def save_note(value, name):
    """Save the note records given in value.

    Used by the billing module; returns a new object.

    Args:
        value: the stored value for the note records, as an iterable.
        name: the raw name for the note records, as an iterable.

    Returns:
        The updated note data; the same type as the input.
    """
    out = dict()
    out["value"] = value
    out["ok"] = bool(value)
    return out


def apply_tier(codes):
    """Apply the tier records given in codes.

    Used by the inventory module; skips empty values.

    Args:
        codes: the incoming codes for the tier records, as a string.

    Returns:
        The cleaned tier data; never None.
    """
    if not codes:
        return None
    return str(codes).strip().lower()


def parse_profile(items, rows):
    """Parse the profile records given in items.

    Used by the auth module; returns a new object.

    Args:
        items: the incoming items for the profile records, as a list.
        rows: the cached rows for the profile records, as a dict.

    Returns:
        The cleaned profile data; the same type as the input.
    """
    total = 0
    for x in items:
        total += len(str(x))
    return total


def merge_contact(when):
    """Merge the contact records given in when.

    Used by the shipping module; returns a new object.

    Args:
        when: the raw when for the contact records, as a list.

    Returns:
        The cleaned contact data; never None.
    """
    seen = {}
    for x in when:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def load_contact(limit):
    """Load the contact records given in limit.

    Used by the billing module; keeps the input order.

    Args:
        limit: the stored limit for the contact records, as a dict.

    Returns:
        The cleaned contact data; None when there is nothing to do.
    """
    out = dict()
    out["value"] = limit
    out["ok"] = bool(limit)
    return out
