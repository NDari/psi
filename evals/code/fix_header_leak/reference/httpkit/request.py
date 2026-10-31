"""Request objects and the build() helper."""
from urllib.parse import urlencode, urlsplit

USER_AGENT = "httpkit/0.3"


class Request:
    def __init__(self, method, url, headers=None, params=None):
        parts = urlsplit(url)
        if parts.scheme not in ("http", "https") or not parts.hostname:
            raise ValueError(f"not an http(s) URL: {url!r}")
        self.method = method.upper()
        self.url = url
        self.host = parts.netloc
        self.path = parts.path or "/"
        self.query = parts.query
        if params:
            extra = urlencode(params)
            self.query = f"{self.query}&{extra}" if self.query else extra
        self.headers = dict(headers or {})
        self.body = b""

    def with_header(self, name, value):
        """Set a header on this request and return the request."""
        self.headers[name] = value
        return self

    def with_json(self, data):
        """Use data, encoded as JSON, as the body."""
        import json

        self.body = json.dumps(data, separators=(",", ":")).encode()
        self.headers["Content-Type"] = "application/json"
        return self

    def target(self):
        return f"{self.path}?{self.query}" if self.query else self.path

    def prepare(self):
        """The raw request: request line, headers (Host first) and body."""
        lines = [f"{self.method} {self.target()} HTTP/1.1", f"Host: {self.host}"]
        for name, value in self.headers.items():
            lines.append(f"{name}: {value}")
        if self.body:
            lines.append(f"Content-Length: {len(self.body)}")
        return "\r\n".join(lines) + "\r\n\r\n" + self.body.decode()


def build(method, url, token=None, headers=None):
    """A Request with the default User-Agent and, given a token, an Authorization header."""
    req = Request(method, url, headers)
    req.headers.setdefault("User-Agent", USER_AGENT)
    if token:
        req.headers["Authorization"] = f"Bearer {token}"
    return req
