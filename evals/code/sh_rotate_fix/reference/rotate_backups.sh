#!/bin/bash
# rotate_backups.sh DIR KEEP: keep the KEEP newest backups in DIR, delete the rest.

usage() {
  echo "usage: rotate_backups.sh DIR KEEP (KEEP a positive integer)" >&2
  exit 2
}

[ $# -eq 2 ] || usage
DIR=$1
KEEP=$2
[[ $KEEP =~ ^[0-9]+$ ]] && ((10#$KEEP > 0)) || usage
[ -d "$DIR" ] || usage
cd -- "$DIR" || exit 2

# The names sort by date, so byte order is oldest first.
backups=()
while IFS= read -r f; do
  backups+=("$f")
done < <(LC_ALL=C ls -1 | grep -E '^backup-[0-9]{4}-[0-9]{2}-[0-9]{2}\.tar\.gz$' | LC_ALL=C sort)

n=$((${#backups[@]} - 10#$KEEP))
for ((i = 0; i < n; i++)); do
  f=${backups[i]}
  [ -f "$f" ] || continue
  rm -- "$f"
  echo "deleted: $f"
done
