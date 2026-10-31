#!/usr/bin/env bash
# Count log lines per level and the noisiest components (port of tally.py).
#
# usage: tally [-l LEVEL] FILE...
set -u
export LC_ALL=C

usage() {
  echo "usage: tally [-l LEVEL] FILE..." >&2
  exit 2
}

level=
if [ "${1-}" = "-l" ]; then
  [ $# -ge 2 ] || usage
  level=$2
  shift 2
fi
[ $# -ge 1 ] || usage

for f in "$@"; do
  if [ ! -f "$f" ] || [ ! -r "$f" ]; then
    echo "tally: cannot read $f" >&2
    exit 1
  fi
done

matches=$(grep -hE '^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2} [A-Z]+ [a-z0-9_-]+: ' -- "$@" |
  awk -v want="$level" '{ comp = $4; sub(/:$/, "", comp); if (want == "" || $3 == want) print $3, comp }')

echo "total $(printf '%s' "$matches" | grep -c .)"
if [ -n "$matches" ]; then
  printf '%s\n' "$matches" | cut -d' ' -f1 | sort | uniq -c | awk '{ print $2, $1 }'
fi
echo "top components:"
if [ -n "$matches" ]; then
  printf '%s\n' "$matches" | cut -d' ' -f2 | sort | uniq -c | awk '{ print $1, $2 }' |
    sort -k1,1nr -k2,2 | head -n 3 | awk '{ print "  " $2, $1 }'
fi
