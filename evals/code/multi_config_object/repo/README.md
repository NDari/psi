# logsift

Summarises access logs of our internal API. A log line looks like

    2024-05-01 10:00:03 INFO GET /api/users 123ms

Run it with

    python3 -m logsift access.log

Tests: `python3 -m unittest discover -s tests`
