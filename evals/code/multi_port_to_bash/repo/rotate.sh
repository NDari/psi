#!/usr/bin/env bash
# rotate.sh DIR DAYS: move *.log files in DIR older than DAYS days into DIR/archive.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: rotate.sh DIR DAYS" >&2
  exit 2
fi
dir=$1
days=$2
case $days in
  '' | *[!0-9]*) echo "rotate: DAYS must be a number" >&2; exit 2 ;;
esac

mkdir -p "$dir/archive"
count=0
while IFS= read -r -d '' f; do
  mv -- "$f" "$dir/archive/"
  count=$((count + 1))
done < <(find "$dir" -maxdepth 1 -name '*.log' -type f -mtime +"$days" -print0)
echo "archived $count file(s)"
