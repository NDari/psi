We found a security problem in httpkit (see README.md): once InventoryClient has made one authenticated call, its health() requests, which should carry no token, also send "Authorization: Bearer ...". The "Idempotency-Key" header from adjust() shows up on later requests too. And when I pass my own dict as build(..., headers=my_headers), my dict gets modified.

Please fix it so every Request has its own headers and build()/Request() never change the dict passed in. The public API (build, Request, with_header, with_json, prepare) should stay the same.
