# latency

Latency summaries from our gateway's access logs.

    python3 -m latency report ACCESS.log [--endpoint PATH] [--by-endpoint]

A log line looks like

    2026-10-20T10:00:01Z GET /api/users 200 123ms

Lines that don't parse are skipped (with a count on stderr). The report
shows the number of requests, the mean, p50, p95 and max latency in ms, and
the error rate (share of status >= 500). Percentiles use the nearest-rank
method: pXX is the value at 1-based position ceil(XX/100 * n) of the sorted
latencies. `--endpoint` restricts the report to one path; `--by-endpoint`
prints one line per path, sorted by path.

Run the tests with `python3 -m unittest`.
