. /evallib/check.sh
# Compile once first: a cold build takes longer than one check may.
timeout 200 go build ./... >/dev/null 2>&1 || true
timeout 200 go vet ./... >/dev/null 2>&1 || true

check tests go test ./...
check incr bash /hidden/says.sh '5 incr' '6 OK'
check hash bash /hidden/says.sh '#[ a: {dup *}] :x let 5 x.a' '25 OK'
