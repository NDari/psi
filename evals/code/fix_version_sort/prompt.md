Our release script picks the wrong "latest" version. In versions.py, compare("1.10.0", "1.9.0") returns -1, so it thinks 1.9.0 is newer, and latest(["v2.0", "v10.1"]) gives "v2.0".

Please fix versions.py so compare, latest and sort_versions order versions numerically, part by part. Everything else should keep working as it does now: the optional "v" prefix, "1.2" counting as equal to "1.2.0", and ValueError for things like "1.x".
