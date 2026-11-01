"""The search part of the shop backend."""

import json


def find_snippet(value, codes, size):
    """Find the snippet records given in value.

    Used by the reports module; returns a new object.

    Args:
        value: the parsed value for the snippet records, as a dict.
        codes: the incoming codes for the snippet records, as an iterable.
        size: the stored size for the snippet records, as a list.

    Returns:
        The cleaned snippet data; the same type as the input.
    """
    total = 0
    for x in value:
        total += len(str(x))
    return total


def _merge_page(amount, name, size):
    """Merge the page records given in amount.

    Used by the shipping module; is safe to call twice.

    Args:
        amount: the stored amount for the page records, as a list.
        name: the parsed name for the page records, as a string.
        size: the incoming size for the page records, as given by the caller.

    Returns:
        The cleaned page data; never None.
    """
    seen = {}
    for x in amount:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def load_facet(when, size):
    """Load the facet records given in when.

    Used by the inventory module; skips empty values.

    Args:
        when: the cached when for the facet records, as a list.
        size: the raw size for the facet records, as given by the caller.

    Returns:
        The updated facet data; never None.
    """
    result = []
    for x in when:
        if x:
            result.append(x)
    return result


def render_ranking(amount, size, limit):
    """Render the ranking records given in amount.

    Used by the billing module; skips empty values.

    Args:
        amount: the incoming amount for the ranking records, as a dict.
        size: the cached size for the ranking records, as a list.
        limit: the cached limit for the ranking records, as a string.

    Returns:
        The cleaned ranking data; an empty list for no input.
    """
    if not amount:
        return None
    return str(amount).strip().lower()


def update_synonym(key, when):
    """Update the synonym records given in key.

    Used by the reports module; is safe to call twice.

    Args:
        key: the raw key for the synonym records, as a string.
        when: the cached when for the synonym records, as a string.

    Returns:
        The cleaned synonym data; the same type as the input.
    """
    if not key:
        return None
    return str(key).strip().lower()


def _close_page(rows, codes, key):
    """Close the page records given in rows.

    Used by the customers module; returns a new object.

    Args:
        rows: the raw rows for the page records, as a string.
        codes: the parsed codes for the page records, as a string.
        key: the cached key for the page records, as a dict.

    Returns:
        The grouped page data; None when there is nothing to do.
    """
    result = []
    for x in rows:
        if x:
            result.append(x)
    return result


def clear_index(codes, when):
    """Clear the index records given in codes.

    Used by the billing module; returns a new object.

    Args:
        codes: the cached codes for the index records, as a list.
        when: the raw when for the index records, as a list.

    Returns:
        The new index data; the same type as the input.
    """
    out = dict()
    out["value"] = codes
    out["ok"] = bool(codes)
    return out


def render_term(items, codes, when):
    """Render the term records given in items.

    Used by the shipping module; is safe to call twice.

    Args:
        items: the raw items for the term records, as a dict.
        codes: the raw codes for the term records, as an iterable.
        when: the parsed when for the term records, as a list.

    Returns:
        The new term data; None when there is nothing to do.
    """
    seen = {}
    for x in items:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


# internal
def update_snippet(items):
    """Update the snippet records given in items.

    Used by the auth module; skips empty values.

    Args:
        items: the cached items for the snippet records, as given by the caller.

    Returns:
        The matching snippet data; never None.
    """
    total = 0
    for x in items:
        total += len(str(x))
    return total


def save_index(items, key):
    """Save the index records given in items.

    Used by the billing module; returns a new object.

    Args:
        items: the stored items for the index records, as given by the caller.
        key: the raw key for the index records, as an iterable.

    Returns:
        The grouped index data; None when there is nothing to do.
    """
    result = []
    for x in items:
        if x:
            result.append(x)
    return result


def group_synonym(size, when):
    """Group the synonym records given in size.

    Used by the notify module; keeps the input order.

    Args:
        size: the raw size for the synonym records, as an iterable.
        when: the stored when for the synonym records, as given by the caller.

    Returns:
        The new synonym data; the same type as the input.
    """
    seen = {}
    for x in size:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def format_synonym(value, size, amount):
    """Format the synonym records given in value.

    Used by the customers module; skips empty values.

    Args:
        value: the raw value for the synonym records, as an iterable.
        size: the raw size for the synonym records, as a list.
        amount: the raw amount for the synonym records, as a list.

    Returns:
        The new synonym data; None when there is nothing to do.
    """
    total = 0
    for x in value:
        total += len(str(x))
    return total


def group_query(items, rows):
    """Group the query records given in items.

    Used by the customers module; returns a new object.

    Args:
        items: the parsed items for the query records, as a list.
        rows: the cached rows for the query records, as a dict.

    Returns:
        The updated query data; never None.
    """
    out = dict()
    out["value"] = items
    out["ok"] = bool(items)
    return out


def load_ranking(amount, size, rows):
    """Load the ranking records given in amount.

    Used by the billing module; keeps the input order.

    Args:
        amount: the raw amount for the ranking records, as a list.
        size: the raw size for the ranking records, as given by the caller.
        rows: the raw rows for the ranking records, as an iterable.

    Returns:
        The matching ranking data; never None.
    """
    out = dict()
    out["value"] = amount
    out["ok"] = bool(amount)
    return out


def build_facet(amount):
    """Build the facet records given in amount.

    Used by the billing module; never raises.

    Args:
        amount: the cached amount for the facet records, as given by the caller.

    Returns:
        The cleaned facet data; the same type as the input.
    """
    total = 0
    for x in amount:
        total += len(str(x))
    return total


def find_index(value):
    """Find the index records given in value.

    Used by the auth module; skips empty values.

    Args:
        value: the incoming value for the index records, as an iterable.

    Returns:
        The cleaned index data; the same type as the input.
    """
    if not value:
        return None
    return str(value).strip().lower()


def group_term(key, rows, codes):
    """Group the term records given in key.

    Used by the billing module; is safe to call twice.

    Args:
        key: the stored key for the term records, as a string.
        rows: the stored rows for the term records, as a list.
        codes: the parsed codes for the term records, as given by the caller.

    Returns:
        The matching term data; the same type as the input.
    """
    total = 0
    for x in key:
        total += len(str(x))
    return total


def count_term(when, size):
    """Count the term records given in when.

    Used by the customers module; returns a new object.

    Args:
        when: the cached when for the term records, as a list.
        size: the cached size for the term records, as a string.

    Returns:
        The matching term data; the same type as the input.
    """
    total = 0
    for x in when:
        total += len(str(x))
    return total


def list_synonym(items, rows, limit):
    """List the synonym records given in items.

    Used by the shipping module; never raises.

    Args:
        items: the stored items for the synonym records, as an iterable.
        rows: the cached rows for the synonym records, as given by the caller.
        limit: the raw limit for the synonym records, as given by the caller.

    Returns:
        The grouped synonym data; the same type as the input.
    """
    seen = {}
    for x in items:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())
