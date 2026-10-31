#!/bin/bash
# archive_logs.sh [-n] [-d DAYS] SRC DEST
# Compress .log files in SRC older than DAYS days (default 7) into DEST.
export LC_ALL=C

usage() {
  echo "usage: archive_logs.sh [-n] [-d DAYS] SRC DEST" >&2
  exit 2
}

DAYS=7
DRY_RUN=0
while getopts ":nd:" opt; do
  case $opt in
    n) DRY_RUN=1 ;;
    d) DAYS=$OPTARG ;;
    *) usage ;;
  esac
done
shift $((OPTIND - 1))
[ $# -eq 2 ] || usage
[[ $DAYS =~ ^[0-9]+$ ]] || usage
SRC=$1
DEST=$2
[ -d "$SRC" ] || usage

files=()
while IFS= read -r -d '' f; do
  files+=("${f##*/}")
done < <(find "$SRC" -mindepth 1 -maxdepth 1 -type f -name '*.log' -mmin +$((10#$DAYS * 1440)) -print0 | sort -z)

status=0
for name in "${files[@]}"; do
  if [ "$DRY_RUN" = 1 ]; then
    echo "would archive: $name"
    continue
  fi
  mkdir -p -- "$DEST" || exit 1
  if [ -e "$DEST/$name.gz" ]; then
    echo "skip: $name" >&2
    status=1
    continue
  fi
  gzip -c -- "$SRC/$name" > "$DEST/$name.gz" && rm -f -- "$SRC/$name" || { status=1; continue; }
  echo "archived: $name"
done
exit $status
