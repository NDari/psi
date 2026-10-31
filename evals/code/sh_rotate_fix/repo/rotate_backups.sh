#!/bin/bash
# rotate_backups.sh DIR KEEP: keep the KEEP newest backups in DIR, delete the rest.

DIR=$1
KEEP=$2

cd $DIR

files=$(ls -t backup-*)
count=0
for f in $files; do
  count=$((count + 1))
  if [ $count -lt $KEEP ]; then
    continue
  fi
  rm $f
  echo "deleted: $f"
done
