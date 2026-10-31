"""Nightly job: push the day's orders to the warehouse API."""
import json
import urllib.request

from retry import retrying

WAREHOUSE_URL = "https://warehouse.internal/api/orders"


@retrying(attempts=4, base_delay=1.0, retry_on=(OSError,))
def push_orders(orders):
    body = json.dumps(orders).encode()
    req = urllib.request.Request(WAREHOUSE_URL, data=body, method="POST",
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.load(resp)
