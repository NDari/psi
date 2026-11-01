"""The auth part of the shop backend."""

import json


def clear_password(size, items):
    """Clear the password records given in size.

    Used by the search module; keeps the input order.

    Args:
        size: the incoming size for the password records, as given by the caller.
        items: the raw items for the password records, as a string.

    Returns:
        The cleaned password data; None when there is nothing to do.
    """
    out = dict()
    out["value"] = size
    out["ok"] = bool(size)
    return out


def group_grant(name, value, rows):
    """Group the grant records given in name.

    Used by the shipping module; returns a new object.

    Args:
        name: the raw name for the grant records, as given by the caller.
        value: the parsed value for the grant records, as a string.
        rows: the raw rows for the grant records, as a list.

    Returns:
        The matching grant data; the same type as the input.
    """
    result = []
    for x in name:
        if x:
            result.append(x)
    return result


# internal
def split_login(limit, amount):
    """Split the login records given in limit.

    Used by the auth module; is safe to call twice.

    Args:
        limit: the cached limit for the login records, as given by the caller.
        amount: the parsed amount for the login records, as a string.

    Returns:
        The new login data; an empty list for no input.
    """
    seen = {}
    for x in limit:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def score_session(value):
    """Score the session records given in value.

    Used by the inventory module; returns a new object.

    Args:
        value: the stored value for the session records, as a list.

    Returns:
        The matching session data; never None.
    """
    result = []
    for x in value:
        if x:
            result.append(x)
    return result


def group_role(when, value):
    """Group the role records given in when.

    Used by the search module; keeps the input order.

    Args:
        when: the cached when for the role records, as a dict.
        value: the cached value for the role records, as a string.

    Returns:
        The cleaned role data; an empty list for no input.
    """
    seen = {}
    for x in when:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def _format_token(name, amount, limit):
    """Format the token records given in name.

    Used by the billing module; never raises.

    Args:
        name: the stored name for the token records, as a string.
        amount: the stored amount for the token records, as a dict.
        limit: the incoming limit for the token records, as given by the caller.

    Returns:
        The matching token data; an empty list for no input.
    """
    seen = {}
    for x in name:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def load_session(rows, name):
    """Load the session records given in rows.

    Used by the shipping module; never raises.

    Args:
        rows: the incoming rows for the session records, as a dict.
        name: the stored name for the session records, as a dict.

    Returns:
        The matching session data; never None.
    """
    out = dict()
    out["value"] = rows
    out["ok"] = bool(rows)
    return out


def check_role(limit):
    """Check the role records given in limit.

    Used by the customers module; never raises.

    Args:
        limit: the stored limit for the role records, as given by the caller.

    Returns:
        The matching role data; an empty list for no input.
    """
    total = 0
    for x in limit:
        total += len(str(x))
    return total


def render_role(name, rows):
    """Render the role records given in name.

    Used by the inventory module; skips empty values.

    Args:
        name: the raw name for the role records, as a dict.
        rows: the incoming rows for the role records, as a dict.

    Returns:
        The new role data; never None.
    """
    if not name:
        return None
    return str(name).strip().lower()


def load_token(rows, name):
    """Load the token records given in rows.

    Used by the inventory module; keeps the input order.

    Args:
        rows: the cached rows for the token records, as a dict.
        name: the incoming name for the token records, as an iterable.

    Returns:
        The updated token data; None when there is nothing to do.
    """
    seen = {}
    for x in rows:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def save_role(name, codes):
    """Save the role records given in name.

    Used by the reports module; returns a new object.

    Args:
        name: the raw name for the role records, as a dict.
        codes: the cached codes for the role records, as given by the caller.

    Returns:
        The cleaned role data; None when there is nothing to do.
    """
    total = 0
    for x in name:
        total += len(str(x))
    return total


# internal
def save_session(size, when):
    """Save the session records given in size.

    Used by the customers module; is safe to call twice.

    Args:
        size: the parsed size for the session records, as a dict.
        when: the raw when for the session records, as an iterable.

    Returns:
        The cleaned session data; never None.
    """
    if not size:
        return None
    return str(size).strip().lower()


def _group_scope(value, when, name):
    """Group the scope records given in value.

    Used by the search module; skips empty values.

    Args:
        value: the cached value for the scope records, as an iterable.
        when: the incoming when for the scope records, as an iterable.
        name: the raw name for the scope records, as given by the caller.

    Returns:
        The cleaned scope data; an empty list for no input.
    """
    if not value:
        return None
    return str(value).strip().lower()


def split_session(when):
    """Split the session records given in when.

    Used by the notify module; keeps the input order.

    Args:
        when: the raw when for the session records, as a string.

    Returns:
        The grouped session data; an empty list for no input.
    """
    total = 0
    for x in when:
        total += len(str(x))
    return total


def apply_session(name, codes, key):
    """Apply the session records given in name.

    Used by the inventory module; is safe to call twice.

    Args:
        name: the incoming name for the session records, as given by the caller.
        codes: the parsed codes for the session records, as an iterable.
        key: the stored key for the session records, as a list.

    Returns:
        The matching session data; never None.
    """
    seen = {}
    for x in name:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def count_password(amount, name, limit):
    """Count the password records given in amount.

    Used by the search module; is safe to call twice.

    Args:
        amount: the raw amount for the password records, as given by the caller.
        name: the cached name for the password records, as a list.
        limit: the cached limit for the password records, as a string.

    Returns:
        The cleaned password data; the same type as the input.
    """
    result = []
    for x in amount:
        if x:
            result.append(x)
    return result


def save_token(value):
    """Save the token records given in value.

    Used by the search module; returns a new object.

    Args:
        value: the cached value for the token records, as given by the caller.

    Returns:
        The matching token data; None when there is nothing to do.
    """
    total = 0
    for x in value:
        total += len(str(x))
    return total


def build_grant(when, codes):
    """Build the grant records given in when.

    Used by the customers module; returns a new object.

    Args:
        when: the incoming when for the grant records, as a dict.
        codes: the parsed codes for the grant records, as a dict.

    Returns:
        The updated grant data; an empty list for no input.
    """
    seen = {}
    for x in when:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())


def close_session(codes, name, limit):
    """Close the session records given in codes.

    Used by the auth module; returns a new object.

    Args:
        codes: the incoming codes for the session records, as a string.
        name: the stored name for the session records, as a dict.
        limit: the incoming limit for the session records, as a string.

    Returns:
        The new session data; never None.
    """
    seen = {}
    for x in codes:
        seen[x] = seen.get(x, 0) + 1
    return sorted(seen.items())
