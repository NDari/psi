#!/usr/bin/env bash
# rename_photos.sh DIR: rename photos by the date and time in their names.
if [ $# -ne 1 ] || [ ! -d "$1" ]; then
  echo "usage: rename_photos.sh DIR" >&2
  exit 2
fi
dir=$1
export LC_ALL=C
for path in "$dir"/*; do
  [ -f "$path" ] && [ ! -L "$path" ] || continue
  old=${path##*/}
  [[ $old =~ ([0-9]{4})([0-9]{2})([0-9]{2})_([0-9]{2})([0-9]{2})([0-9]{2}) ]] || continue
  [[ $old == *.* ]] || continue
  ext=${old##*.}
  ext=${ext,,}
  m=("${BASH_REMATCH[@]}")
  new="${m[1]}-${m[2]}-${m[3]}_${m[4]}-${m[5]}-${m[6]}.$ext"
  [ "$new" = "$old" ] && continue
  if [ -e "$dir/$new" ]; then
    echo "skip: $old" >&2
    continue
  fi
  mv -- "$path" "$dir/$new"
  echo "$old -> $new"
done
