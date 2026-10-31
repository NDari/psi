Our new proxy writes its access log as JSON lines, and logstats should read those too. Each line is a JSON object such as

    {"host": "10.0.0.1", "time": "2026-10-10T13:55:36+02:00", "method": "GET", "path": "/a.gif?x=1", "status": 200, "bytes": 2326}

- Add `parse_jsonl_line(line)` to logstats/parse.py, returning an Entry like `parse_clf_line` does, or None for a malformed line. "time" is ISO 8601 as `datetime.fromisoformat` reads it and must have a UTC offset; "status" is an integer; "bytes" is an integer, or null or absent meaning 0; "path" goes through the existing `strip_query`, as for CLF. Anything else (not JSON, not an object, a missing key, a wrong type, a time without offset) is malformed.
- Add a `--format {auto,clf,jsonl}` option, default auto. With auto, each file's format is decided by its first non-blank line: JSON lines if it starts with "{", CLF otherwise. Files of different formats can be given together.
- Malformed lines count as "skipped" and blank lines are ignored, in both formats, as for CLF today. The report stays the same.
