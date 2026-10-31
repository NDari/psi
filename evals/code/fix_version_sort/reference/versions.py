"""Helpers for comparing release version strings such as "1.4.2" or "v2.0".

A version is a dot-separated list of non-negative integers, optionally
prefixed with "v". Trailing zero parts do not matter: "1.2" == "1.2.0".
"""


def parse_version(text):
    """Turn "v1.4.2" into a tuple that sorts the way versions do."""
    text = text.strip()
    if text.startswith(("v", "V")):
        text = text[1:]
    parts = text.split(".")
    if not all(p.isdigit() for p in parts):
        raise ValueError(f"bad version: {text!r}")
    # "1.2.0" and "1.2" are the same release
    while len(parts) > 1 and parts[-1].strip("0") == "":
        parts.pop()
    return tuple(int(p) for p in parts)


def compare(a, b):
    """Return -1 if a is older than b, 0 if they are the same, 1 if newer."""
    pa, pb = parse_version(a), parse_version(b)
    return (pa > pb) - (pa < pb)


def latest(versions):
    """Return the newest version string from an iterable of version strings."""
    return max(versions, key=parse_version)


def sort_versions(versions):
    """Return the version strings sorted oldest first."""
    return sorted(versions, key=parse_version)


if __name__ == "__main__":
    import sys

    for v in sort_versions(sys.argv[1:]):
        print(v)
