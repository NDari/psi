"""Clients for our internal services, built on httpkit.build."""
from .request import build


class InventoryClient:
    def __init__(self, base_url, send, token=None):
        """send(request) performs the request and returns (status, body)."""
        self.base_url = base_url.rstrip("/")
        self.send = send
        self.token = token

    def item(self, sku):
        return self.send(build("GET", f"{self.base_url}/items/{sku}", token=self.token))

    def health(self):
        # the health endpoint is public: no token
        return self.send(build("GET", f"{self.base_url}/health"))

    def adjust(self, sku, delta, request_id):
        req = build("POST", f"{self.base_url}/items/{sku}/adjust", token=self.token)
        req.with_header("Idempotency-Key", request_id).with_json({"delta": delta})
        return self.send(req)
