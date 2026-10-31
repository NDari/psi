# httpkit

A tiny HTTP/1.1 request builder used by our internal service clients.

    from httpkit import build
    req = build("GET", "https://api.example.com/v1/items?limit=5", token="abc")
    req.with_header("Accept", "application/json")
    print(req.prepare())    # raw request text

`build(method, url, token=None, headers={})` returns a `Request`. Every
request gets a `User-Agent` header (unless one is given) and, when a token is
passed, `Authorization: Bearer <token>`.
