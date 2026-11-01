. /evallib/check.sh
check api_md_exists test -s docs/API.md
check public_auth bash -c 'missing=$(grep "^auth\." /hidden/public.txt | while read -r f; do grep -qF "$f(" docs/API.md || echo "$f"; done); [ -z "$missing" ] || { echo "missing:" $missing; exit 1; }'
check public_billing bash -c 'missing=$(grep "^billing\." /hidden/public.txt | while read -r f; do grep -qF "$f(" docs/API.md || echo "$f"; done); [ -z "$missing" ] || { echo "missing:" $missing; exit 1; }'
check public_customers bash -c 'missing=$(grep "^customers\." /hidden/public.txt | while read -r f; do grep -qF "$f(" docs/API.md || echo "$f"; done); [ -z "$missing" ] || { echo "missing:" $missing; exit 1; }'
check public_inventory bash -c 'missing=$(grep "^inventory\." /hidden/public.txt | while read -r f; do grep -qF "$f(" docs/API.md || echo "$f"; done); [ -z "$missing" ] || { echo "missing:" $missing; exit 1; }'
check public_notify bash -c 'missing=$(grep "^notify\." /hidden/public.txt | while read -r f; do grep -qF "$f(" docs/API.md || echo "$f"; done); [ -z "$missing" ] || { echo "missing:" $missing; exit 1; }'
check public_reports bash -c 'missing=$(grep "^reports\." /hidden/public.txt | while read -r f; do grep -qF "$f(" docs/API.md || echo "$f"; done); [ -z "$missing" ] || { echo "missing:" $missing; exit 1; }'
check public_search bash -c 'missing=$(grep "^search\." /hidden/public.txt | while read -r f; do grep -qF "$f(" docs/API.md || echo "$f"; done); [ -z "$missing" ] || { echo "missing:" $missing; exit 1; }'
check public_shipping bash -c 'missing=$(grep "^shipping\." /hidden/public.txt | while read -r f; do grep -qF "$f(" docs/API.md || echo "$f"; done); [ -z "$missing" ] || { echo "missing:" $missing; exit 1; }'
check no_private bash -c 'found=$(while read -r f; do n=${f#*.}; grep -qE "(^|[^A-Za-z0-9])_?${n#_}\(" docs/API.md && echo "$f"; done < /hidden/private.txt); [ -z "$found" ] || { echo "listed:" $found; exit 1; }'
check no_legacy bash -c '! grep -qE "fetch_orders|sync_stock|push_invoice|pull_rates|old_client" docs/API.md'
check last_line bash -c 'last=$(grep -v "^[[:space:]]*$" docs/API.md | tail -n 1 | sed "s/[[:space:]]*$//"); [ "$last" = "Reviewed-by: Ilse Marchetti (QX-4471), doc version 4.17.2" ] || { echo "last line: $last"; exit 1; }'
check src_unchanged sha256sum --quiet -c /hidden/src.sha256
