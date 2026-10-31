#!/bin/bash
# archive_logs.sh [-n] [-d DAYS] SRC DEST
# Compress .log files in SRC older than DAYS days (default 7) into DEST.

DAYS=7
DRY_RUN=0
while getopts "nd" opt; do
  case $opt in
    n) DRY_RUN=1 ;;
    d) DAYS=$OPTARG ;;
  esac
done
shift $((OPTIND - 1))
SRC=$1
DEST=$2

for f in $(find $SRC -name "*.log" -mtime -$DAYS); do
  name=$(basename $f)
  if [ $DRY_RUN = 1 ]; then
    echo "would archive: $name"
  fi
  gzip $f
  mv $f.gz $DEST/
  echo "archived: $name"
done
