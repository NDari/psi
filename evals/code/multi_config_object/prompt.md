logsift has its settings hard-coded all over the package: the 400 character line cut in reader.py, the timestamp format "%Y-%m-%d %H:%M:%S" in parser.py and report.py, the 500 ms slow threshold in analyze.py and report.py, and the 5 slowest paths in report.py. Move them into one config object.

- New module `logsift/config.py` with a dataclass `Config` whose fields and defaults are `max_line_length=400`, `time_format="%Y-%m-%d %H:%M:%S"`, `slow_ms=500`, `top_n=5`, and a function `load_config(path)` that reads a JSON object holding any subset of those fields and returns a `Config` (missing fields keep their defaults; an unknown key raises `ValueError`).
- These functions get a new last parameter `config=None` (None means `Config()`) and use it instead of the literals: `read_lines(path, config=None)`, `parse_line(line, config=None)`, `parse_all(lines, config=None)`, `slow_requests(entries, config=None)`, `render(entries, skipped=0, config=None)`. The report's "slow requests (>= N ms)" line shows the configured threshold.
- The CLI gets `--config PATH`: `python3 -m logsift FILE --config settings.json`. Without it, output stays exactly as today.

Afterwards those values should appear only in config.py: no other module should contain 400, 500 or the format string. Existing tests are in tests/ (`python3 -m unittest discover -s tests`).
