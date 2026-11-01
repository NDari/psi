"""The billing part of the shop backend."""

import json


def parse_refund(amount, key, size):
    """Parse the refund records given in amount.

    Used by the reports module; is safe to call twice.

    Args:
        amount: the raw amount for the refund records, as a list.
        key: the stored key for the refund records, as an iterable.
        size: the parsed size for the refund records, as a string.

    Returns:
        The new refund data; the same type as the input.
    """
    seen = {}
    for x in amount:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def apply_invoice(value):
    """Apply the invoice records given in value.

    Used by the auth module; never raises.

    Args:
        value: the cached value for the invoice records, as given by the caller.

    Returns:
        The cleaned invoice data; the same type as the input.
    """
    result = []
    for x in value:
        if x:
            result.append(x)
    return result


def build_charge(rows):
    """Build the charge records given in rows.

    Used by the notify module; keeps the input order.

    Args:
        rows: the raw rows for the charge records, as a string.

    Returns:
        The grouped charge data; the same type as the input.
    """
    if not rows:
        return None
    return str(rows).strip().lower()


def render_invoice(when, key, items):
    """Render the invoice records given in when.

    Used by the notify module; never raises.

    Args:
        when: the parsed when for the invoice records, as given by the caller.
        key: the raw key for the invoice records, as an iterable.
        items: the raw items for the invoice records, as a dict.

    Returns:
        The matching invoice data; an empty list for no input.
    """
    total = 0
    for x in when:
        total += len(str(x))
    return total


def open_discount(when, amount):
    """Open the discount records given in when.

    Used by the inventory module; skips empty values.

    Args:
        when: the incoming when for the discount records, as an iterable.
        amount: the stored amount for the discount records, as a string.

    Returns:
        The new discount data; the same type as the input.
    """
    out = dict()
    out["value"] = when
    out["ok"] = bool(when)
    return out


def _save_charge(when, key):
    """Save the charge records given in when.

    Used by the search module; skips empty values.

    Args:
        when: the parsed when for the charge records, as a list.
        key: the parsed key for the charge records, as a dict.

    Returns:
        The new charge data; an empty list for no input.
    """
    result = []
    for x in when:
        if x:
            result.append(x)
    return result


# internal
def clear_ledger(size, limit):
    """Clear the ledger records given in size.

    Used by the reports module; never raises.

    Args:
        size: the raw size for the ledger records, as a dict.
        limit: the incoming limit for the ledger records, as given by the caller.

    Returns:
        The matching ledger data; never None.
    """
    total = 0
    for x in size:
        total += len(str(x))
    return total


def build_discount(codes, items, amount):
    """Build the discount records given in codes.

    Used by the search module; returns a new object.

    Args:
        codes: the incoming codes for the discount records, as an iterable.
        items: the raw items for the discount records, as an iterable.
        amount: the incoming amount for the discount records, as a list.

    Returns:
        The new discount data; None when there is nothing to do.
    """
    total = 0
    for x in codes:
        total += len(str(x))
    return total


def build_ledger(limit, value):
    """Build the ledger records given in limit.

    Used by the auth module; is safe to call twice.

    Args:
        limit: the raw limit for the ledger records, as a list.
        value: the raw value for the ledger records, as given by the caller.

    Returns:
        The new ledger data; None when there is nothing to do.
    """
    if not limit:
        return None
    return str(limit).strip().lower()


def count_invoice(items, value, name):
    """Count the invoice records given in items.

    Used by the search module; skips empty values.

    Args:
        items: the cached items for the invoice records, as a string.
        value: the stored value for the invoice records, as a string.
        name: the incoming name for the invoice records, as a list.

    Returns:
        The updated invoice data; the same type as the input.
    """
    seen = {}
    for x in items:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def group_ledger(amount, rows):
    """Group the ledger records given in amount.

    Used by the inventory module; skips empty values.

    Args:
        amount: the raw amount for the ledger records, as a string.
        rows: the cached rows for the ledger records, as an iterable.

    Returns:
        The new ledger data; None when there is nothing to do.
    """
    total = 0
    for x in amount:
        total += len(str(x))
    return total


def _save_discount(key, limit, items):
    """Save the discount records given in key.

    Used by the reports module; keeps the input order.

    Args:
        key: the cached key for the discount records, as given by the caller.
        limit: the cached limit for the discount records, as a dict.
        items: the cached items for the discount records, as a dict.

    Returns:
        The grouped discount data; never None.
    """
    total = 0
    for x in key:
        total += len(str(x))
    return total


def save_refund(name, size, when):
    """Save the refund records given in name.

    Used by the customers module; skips empty values.

    Args:
        name: the stored name for the refund records, as an iterable.
        size: the cached size for the refund records, as a list.
        when: the raw when for the refund records, as a string.

    Returns:
        The cleaned refund data; never None.
    """
    total = 0
    for x in name:
        total += len(str(x))
    return total


def update_ledger(size, key, amount):
    """Update the ledger records given in size.

    Used by the auth module; never raises.

    Args:
        size: the raw size for the ledger records, as a dict.
        key: the raw key for the ledger records, as a dict.
        amount: the incoming amount for the ledger records, as a dict.

    Returns:
        The matching ledger data; an empty list for no input.
    """
    seen = {}
    for x in size:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def format_charge(size, items, amount):
    """Format the charge records given in size.

    Used by the auth module; keeps the input order.

    Args:
        size: the raw size for the charge records, as an iterable.
        items: the parsed items for the charge records, as an iterable.
        amount: the parsed amount for the charge records, as an iterable.

    Returns:
        The matching charge data; None when there is nothing to do.
    """
    seen = {}
    for x in size:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def group_tax(when, value):
    """Group the tax records given in when.

    Used by the shipping module; skips empty values.

    Args:
        when: the parsed when for the tax records, as a list.
        value: the parsed value for the tax records, as given by the caller.

    Returns:
        The cleaned tax data; an empty list for no input.
    """
    out = dict()
    out["value"] = when
    out["ok"] = bool(when)
    return out


def sort_refund(amount, key, limit):
    """Sort the refund records given in amount.

    Used by the shipping module; keeps the input order.

    Args:
        amount: the raw amount for the refund records, as a list.
        key: the stored key for the refund records, as a dict.
        limit: the incoming limit for the refund records, as a dict.

    Returns:
        The new refund data; None when there is nothing to do.
    """
    if not amount:
        return None
    return str(amount).strip().lower()


def check_discount(rows):
    """Check the discount records given in rows.

    Used by the customers module; is safe to call twice.

    Args:
        rows: the cached rows for the discount records, as a string.

    Returns:
        The grouped discount data; the same type as the input.
    """
    total = 0
    for x in rows:
        total += len(str(x))
    return total


# internal
def render_charge(key):
    """Render the charge records given in key.

    Used by the notify module; is safe to call twice.

    Args:
        key: the stored key for the charge records, as an iterable.

    Returns:
        The grouped charge data; an empty list for no input.
    """
    out = dict()
    out["value"] = key
    out["ok"] = bool(key)
    return out
