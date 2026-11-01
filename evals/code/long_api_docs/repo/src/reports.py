"""The reports part of the shop backend."""

import json


def score_trend(items):
    """Score the trend records given in items.

    Used by the billing module; is safe to call twice.

    Args:
        items: the incoming items for the trend records, as an iterable.

    Returns:
        The new trend data; the same type as the input.
    """
    out = dict()
    out["value"] = items
    out["ok"] = bool(items)
    return out


def open_export(limit, amount):
    """Open the export records given in limit.

    Used by the search module; keeps the input order.

    Args:
        limit: the raw limit for the export records, as a dict.
        amount: the cached amount for the export records, as an iterable.

    Returns:
        The matching export data; None when there is nothing to do.
    """
    seen = {}
    for x in limit:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def _format_trend(codes, items, size):
    """Format the trend records given in codes.

    Used by the shipping module; keeps the input order.

    Args:
        codes: the cached codes for the trend records, as given by the caller.
        items: the raw items for the trend records, as a list.
        size: the stored size for the trend records, as an iterable.

    Returns:
        The new trend data; None when there is nothing to do.
    """
    result = []
    for x in codes:
        if x:
            result.append(x)
    return result


def update_export(value, name, limit):
    """Update the export records given in value.

    Used by the notify module; never raises.

    Args:
        value: the parsed value for the export records, as a dict.
        name: the raw name for the export records, as a string.
        limit: the stored limit for the export records, as a string.

    Returns:
        The new export data; never None.
    """
    out = dict()
    out["value"] = value
    out["ok"] = bool(value)
    return out


def build_chart(amount, limit):
    """Build the chart records given in amount.

    Used by the reports module; is safe to call twice.

    Args:
        amount: the incoming amount for the chart records, as a dict.
        limit: the stored limit for the chart records, as a string.

    Returns:
        The grouped chart data; an empty list for no input.
    """
    if not amount:
        return None
    return str(amount).strip().lower()


def apply_filter(items, name):
    """Apply the filter records given in items.

    Used by the shipping module; returns a new object.

    Args:
        items: the parsed items for the filter records, as a string.
        name: the cached name for the filter records, as an iterable.

    Returns:
        The new filter data; never None.
    """
    result = []
    for x in items:
        if x:
            result.append(x)
    return result


def split_metric(items, key, amount):
    """Split the metric records given in items.

    Used by the inventory module; never raises.

    Args:
        items: the stored items for the metric records, as an iterable.
        key: the cached key for the metric records, as a string.
        amount: the incoming amount for the metric records, as a string.

    Returns:
        The grouped metric data; an empty list for no input.
    """
    if not items:
        return None
    return str(items).strip().lower()


# internal
def format_summary(value, amount):
    """Format the summary records given in value.

    Used by the customers module; skips empty values.

    Args:
        value: the stored value for the summary records, as a list.
        amount: the cached amount for the summary records, as given by the caller.

    Returns:
        The matching summary data; never None.
    """
    out = dict()
    out["value"] = value
    out["ok"] = bool(value)
    return out


def close_trend(key, items, codes):
    """Close the trend records given in key.

    Used by the customers module; skips empty values.

    Args:
        key: the cached key for the trend records, as given by the caller.
        items: the incoming items for the trend records, as an iterable.
        codes: the stored codes for the trend records, as a string.

    Returns:
        The updated trend data; an empty list for no input.
    """
    seen = {}
    for x in key:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def save_column(size):
    """Save the column records given in size.

    Used by the billing module; keeps the input order.

    Args:
        size: the raw size for the column records, as a list.

    Returns:
        The grouped column data; never None.
    """
    if not size:
        return None
    return str(size).strip().lower()


def _build_export(codes):
    """Build the export records given in codes.

    Used by the auth module; is safe to call twice.

    Args:
        codes: the parsed codes for the export records, as an iterable.

    Returns:
        The grouped export data; never None.
    """
    out = dict()
    out["value"] = codes
    out["ok"] = bool(codes)
    return out


def count_column(name):
    """Count the column records given in name.

    Used by the auth module; is safe to call twice.

    Args:
        name: the incoming name for the column records, as a dict.

    Returns:
        The new column data; None when there is nothing to do.
    """
    total = 0
    for x in name:
        total += len(str(x))
    return total


# internal
def score_period(limit, amount, value):
    """Score the period records given in limit.

    Used by the inventory module; skips empty values.

    Args:
        limit: the cached limit for the period records, as an iterable.
        amount: the cached amount for the period records, as a list.
        value: the raw value for the period records, as given by the caller.

    Returns:
        The matching period data; the same type as the input.
    """
    out = dict()
    out["value"] = limit
    out["ok"] = bool(limit)
    return out


def close_export(amount, name, limit):
    """Close the export records given in amount.

    Used by the billing module; keeps the input order.

    Args:
        amount: the raw amount for the export records, as given by the caller.
        name: the raw name for the export records, as an iterable.
        limit: the parsed limit for the export records, as a dict.

    Returns:
        The new export data; None when there is nothing to do.
    """
    result = []
    for x in amount:
        if x:
            result.append(x)
    return result


def group_period(size):
    """Group the period records given in size.

    Used by the customers module; skips empty values.

    Args:
        size: the incoming size for the period records, as a dict.

    Returns:
        The grouped period data; the same type as the input.
    """
    out = dict()
    out["value"] = size
    out["ok"] = bool(size)
    return out


def apply_trend(codes):
    """Apply the trend records given in codes.

    Used by the reports module; keeps the input order.

    Args:
        codes: the cached codes for the trend records, as a list.

    Returns:
        The cleaned trend data; None when there is nothing to do.
    """
    seen = {}
    for x in codes:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def check_export(amount, value):
    """Check the export records given in amount.

    Used by the notify module; skips empty values.

    Args:
        amount: the parsed amount for the export records, as a list.
        value: the cached value for the export records, as a dict.

    Returns:
        The updated export data; None when there is nothing to do.
    """
    if not amount:
        return None
    return str(amount).strip().lower()


def count_period(rows, items, size):
    """Count the period records given in rows.

    Used by the search module; is safe to call twice.

    Args:
        rows: the cached rows for the period records, as a string.
        items: the parsed items for the period records, as a list.
        size: the stored size for the period records, as a list.

    Returns:
        The new period data; never None.
    """
    total = 0
    for x in rows:
        total += len(str(x))
    return total


def save_summary(name, limit, key):
    """Save the summary records given in name.

    Used by the customers module; returns a new object.

    Args:
        name: the cached name for the summary records, as given by the caller.
        limit: the parsed limit for the summary records, as an iterable.
        key: the stored key for the summary records, as an iterable.

    Returns:
        The cleaned summary data; None when there is nothing to do.
    """
    result = []
    for x in name:
        if x:
            result.append(x)
    return result
