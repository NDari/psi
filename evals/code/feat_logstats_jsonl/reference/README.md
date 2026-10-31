# logstats

Summaries of web server access logs in the Common Log Format or as JSON
lines (one object per line with host, time, method, path, status, bytes).
The format is detected per file; force it with `--format clf` or
`--format jsonl`.

    python3 -m logstats access.log access.log.1
    python3 -m logstats --since 2026-10-10 --status 5xx --top 10 access.log

Output:

    requests: 1234
    skipped: 2
    bytes: 5520311
    status: 2xx=1100 3xx=80 4xx=50 5xx=4
    top paths:
        310 /
         92 /api/items
    top hosts:
        200 10.0.0.7
    busiest hour: 2026-10-10 13:00 UTC (180 requests)

Lines that cannot be parsed are counted under "skipped". Hours are in UTC.

Run the tests with `python3 -m unittest`.
