import unittest

from httpkit import Request, build
from httpkit.client import InventoryClient


class TestHiddenHttpkit(unittest.TestCase):
    def test_token_does_not_leak(self):
        build("GET", "https://a.example/x", token="secret")
        req = build("GET", "https://a.example/y")
        self.assertNotIn("Authorization", req.headers)

    def test_with_header_does_not_leak(self):
        build("GET", "https://a.example/x").with_header("X-Trace", "1")
        self.assertNotIn("X-Trace", build("GET", "https://a.example/y").headers)
        Request("GET", "https://a.example/x").with_header("X-Other", "2")
        self.assertNotIn("X-Other", Request("GET", "https://a.example/z").headers)

    def test_caller_dict_not_modified(self):
        mine = {"Accept": "text/plain"}
        req = build("GET", "https://a.example/x", token="t", headers=mine)
        req.with_header("X-More", "1")
        self.assertEqual(mine, {"Accept": "text/plain"})
        self.assertEqual(req.headers["Accept"], "text/plain")
        self.assertEqual(req.headers["Authorization"], "Bearer t")

    def test_request_dict_not_modified(self):
        mine = {"Accept": "*/*"}
        req = Request("POST", "http://a.example/x", mine).with_json({"a": 1})
        self.assertEqual(mine, {"Accept": "*/*"})
        self.assertEqual(req.headers["Content-Type"], "application/json")

    def test_client_health_has_no_token(self):
        sent = []
        client = InventoryClient("https://inv.example/", lambda r: sent.append(r) or (200, ""), token="tok")
        client.item("A1")
        client.adjust("A1", -2, "req-7")
        client.health()
        self.assertEqual(sent[0].headers.get("Authorization"), "Bearer tok")
        self.assertNotIn("Idempotency-Key", sent[0].headers)
        self.assertEqual(sent[1].headers.get("Idempotency-Key"), "req-7")
        self.assertNotIn("Authorization", sent[2].headers)
        self.assertNotIn("Idempotency-Key", sent[2].headers)
        self.assertNotIn("Content-Type", sent[2].headers)

    def test_existing_behaviour(self):
        req = build("get", "https://api.example.com/v1/items?limit=5", token="abc")
        req.with_header("Accept", "application/json")
        self.assertEqual(req.prepare(),
                         "GET /v1/items?limit=5 HTTP/1.1\r\nHost: api.example.com\r\n"
                         "User-Agent: httpkit/0.3\r\nAuthorization: Bearer abc\r\n"
                         "Accept: application/json\r\n\r\n")
        req = build("POST", "http://h.example:8080", headers={"User-Agent": "me"}).with_json({"x": [1, 2]})
        self.assertEqual(req.prepare(),
                         "POST / HTTP/1.1\r\nHost: h.example:8080\r\nUser-Agent: me\r\n"
                         "Content-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"x\":[1,2]}")
        self.assertEqual(Request("GET", "https://h.example/p?a=1", params={"b": "2 3"}).target(), "/p?a=1&b=2+3")
        with self.assertRaises(ValueError):
            Request("GET", "ftp://h.example/")


if __name__ == "__main__":
    unittest.main()
