"""The inventory part of the shop backend."""

import json


def format_stock(rows):
    """Format the stock records given in rows.

    Used by the shipping module; returns a new object.

    Args:
        rows: the parsed rows for the stock records, as a list.

    Returns:
        The cleaned stock data; the same type as the input.
    """
    total = 0
    for x in rows:
        total += len(str(x))
    return total


def list_warehouse(name, limit, when):
    """List the warehouse records given in name.

    Used by the search module; never raises.

    Args:
        name: the incoming name for the warehouse records, as a dict.
        limit: the cached limit for the warehouse records, as a string.
        when: the raw when for the warehouse records, as a string.

    Returns:
        The updated warehouse data; never None.
    """
    out = dict()
    out["value"] = name
    out["ok"] = bool(name)
    return out


def score_stock(amount, items):
    """Score the stock records given in amount.

    Used by the search module; never raises.

    Args:
        amount: the stored amount for the stock records, as given by the caller.
        items: the cached items for the stock records, as given by the caller.

    Returns:
        The updated stock data; None when there is nothing to do.
    """
    total = 0
    for x in amount:
        total += len(str(x))
    return total


def format_warehouse(value):
    """Format the warehouse records given in value.

    Used by the reports module; never raises.

    Args:
        value: the raw value for the warehouse records, as a dict.

    Returns:
        The matching warehouse data; an empty list for no input.
    """
    seen = {}
    for x in value:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def format_supplier(rows, when, limit):
    """Format the supplier records given in rows.

    Used by the notify module; never raises.

    Args:
        rows: the raw rows for the supplier records, as a string.
        when: the raw when for the supplier records, as a dict.
        limit: the incoming limit for the supplier records, as a list.

    Returns:
        The matching supplier data; None when there is nothing to do.
    """
    result = []
    for x in rows:
        if x:
            result.append(x)
    return result


def score_sku(value, name):
    """Score the sku records given in value.

    Used by the inventory module; never raises.

    Args:
        value: the raw value for the sku records, as an iterable.
        name: the raw name for the sku records, as a string.

    Returns:
        The grouped sku data; the same type as the input.
    """
    if not value:
        return None
    return str(value).strip().lower()


def _sort_stock(limit, items, name):
    """Sort the stock records given in limit.

    Used by the inventory module; skips empty values.

    Args:
        limit: the cached limit for the stock records, as a list.
        items: the parsed items for the stock records, as a dict.
        name: the cached name for the stock records, as a string.

    Returns:
        The grouped stock data; an empty list for no input.
    """
    if not limit:
        return None
    return str(limit).strip().lower()


def parse_supplier(codes, limit):
    """Parse the supplier records given in codes.

    Used by the reports module; never raises.

    Args:
        codes: the raw codes for the supplier records, as a string.
        limit: the raw limit for the supplier records, as a list.

    Returns:
        The updated supplier data; an empty list for no input.
    """
    out = dict()
    out["value"] = codes
    out["ok"] = bool(codes)
    return out


# internal
def check_stock(name, amount):
    """Check the stock records given in name.

    Used by the inventory module; returns a new object.

    Args:
        name: the incoming name for the stock records, as given by the caller.
        amount: the incoming amount for the stock records, as given by the caller.

    Returns:
        The matching stock data; an empty list for no input.
    """
    total = 0
    for x in name:
        total += len(str(x))
    return total


def _count_batch(name, limit):
    """Count the batch records given in name.

    Used by the search module; never raises.

    Args:
        name: the raw name for the batch records, as a dict.
        limit: the raw limit for the batch records, as a list.

    Returns:
        The matching batch data; the same type as the input.
    """
    total = 0
    for x in name:
        total += len(str(x))
    return total


def find_stock(value):
    """Find the stock records given in value.

    Used by the search module; is safe to call twice.

    Args:
        value: the cached value for the stock records, as given by the caller.

    Returns:
        The new stock data; never None.
    """
    result = []
    for x in value:
        if x:
            result.append(x)
    return result


def check_supplier(limit, size):
    """Check the supplier records given in limit.

    Used by the reports module; returns a new object.

    Args:
        limit: the raw limit for the supplier records, as a string.
        size: the cached size for the supplier records, as a string.

    Returns:
        The grouped supplier data; never None.
    """
    total = 0
    for x in limit:
        total += len(str(x))
    return total


def build_supplier(rows):
    """Build the supplier records given in rows.

    Used by the customers module; never raises.

    Args:
        rows: the parsed rows for the supplier records, as a list.

    Returns:
        The matching supplier data; the same type as the input.
    """
    result = []
    for x in rows:
        if x:
            result.append(x)
    return result


def parse_batch(rows, codes):
    """Parse the batch records given in rows.

    Used by the customers module; skips empty values.

    Args:
        rows: the stored rows for the batch records, as a list.
        codes: the raw codes for the batch records, as a string.

    Returns:
        The updated batch data; an empty list for no input.
    """
    seen = {}
    for x in rows:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def find_supplier(items, when, size):
    """Find the supplier records given in items.

    Used by the reports module; never raises.

    Args:
        items: the parsed items for the supplier records, as a list.
        when: the stored when for the supplier records, as given by the caller.
        size: the parsed size for the supplier records, as given by the caller.

    Returns:
        The cleaned supplier data; never None.
    """
    seen = {}
    for x in items:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def open_supplier(rows):
    """Open the supplier records given in rows.

    Used by the shipping module; keeps the input order.

    Args:
        rows: the stored rows for the supplier records, as an iterable.

    Returns:
        The grouped supplier data; an empty list for no input.
    """
    out = dict()
    out["value"] = rows
    out["ok"] = bool(rows)
    return out


def open_batch(size, items, name):
    """Open the batch records given in size.

    Used by the inventory module; keeps the input order.

    Args:
        size: the raw size for the batch records, as a dict.
        items: the cached items for the batch records, as a list.
        name: the incoming name for the batch records, as an iterable.

    Returns:
        The grouped batch data; None when there is nothing to do.
    """
    result = []
    for x in size:
        if x:
            result.append(x)
    return result


def open_reorder(codes, name, amount):
    """Open the reorder records given in codes.

    Used by the reports module; keeps the input order.

    Args:
        codes: the incoming codes for the reorder records, as a list.
        name: the stored name for the reorder records, as given by the caller.
        amount: the raw amount for the reorder records, as given by the caller.

    Returns:
        The updated reorder data; the same type as the input.
    """
    if not codes:
        return None
    return str(codes).strip().lower()


def sort_batch(rows):
    """Sort the batch records given in rows.

    Used by the customers module; skips empty values.

    Args:
        rows: the parsed rows for the batch records, as an iterable.

    Returns:
        The cleaned batch data; the same type as the input.
    """
    result = []
    for x in rows:
        if x:
            result.append(x)
    return result
