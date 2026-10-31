`python3 -m latency report access.log` prints "p50 n/a, p95 n/a, max 0ms" although the request count and the mean look right. The same happens with --endpoint and --by-endpoint. The unit tests in tests/ all pass (python3 -m unittest), so I don't see what's going on. See README.md for what the report should show.

Please fix it. Keep the output format, the command line, and the Python API (read_log, summarize, only_endpoint, by_endpoint) the same.
