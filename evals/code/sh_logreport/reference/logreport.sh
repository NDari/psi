#!/usr/bin/env bash
# logreport.sh FILE: summarize an access log in combined log format.
if [ $# -ne 1 ] || [ ! -r "$1" ]; then
  echo "usage: logreport.sh FILE" >&2
  exit 2
fi
export LC_ALL=C
awk 'NF > 0 {
  n++
  ip[$1] = 1
  if ($10 != "-") bytes += $10
  status[$9]++
  p = $7; sub(/\?.*/, "", p)
  paths[p]++
}
END {
  for (k in ip) u++
  printf "requests: %d\nunique ips: %d\nbytes: %d\n", n, u, bytes
  for (k in status) print "S", k, status[k]
  for (k in paths) print "P", paths[k], k
}' "$1" > "${TMPDIR:-/tmp}/logreport.$$"
head -n 3 "${TMPDIR:-/tmp}/logreport.$$"
echo "status:"
grep '^S ' "${TMPDIR:-/tmp}/logreport.$$" | sort -k2,2n | awk '{print "  " $2 ": " $3}'
echo "top paths:"
grep '^P ' "${TMPDIR:-/tmp}/logreport.$$" | sort -k2,2nr -k3,3 | head -n 3 | awk '{print "  " $2 " " $3}'
rm -f "${TMPDIR:-/tmp}/logreport.$$"
