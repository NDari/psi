#!/usr/bin/env bash
# iniget.sh FILE SECTION KEY: print the value of KEY in [SECTION] of an INI file.
if [ $# -ne 3 ]; then
  echo "usage: iniget.sh FILE SECTION KEY" >&2
  exit 2
fi
file=$1 section=$2 key=$3
if [ ! -f "$file" ]; then
  echo "iniget.sh: $file: no such file" >&2
  exit 2
fi
awk -v want_s="$section" -v want_k="$key" '
function trim(s) { sub(/^[ \t\r]+/, "", s); sub(/[ \t\r]+$/, "", s); return s }
{
  line = trim($0)
  if (line == "" || substr(line, 1, 1) == ";" || substr(line, 1, 1) == "#") next
  if (substr(line, 1, 1) == "[" && substr(line, length(line), 1) == "]") {
    cur = substr(line, 2, length(line) - 2)
    next
  }
  i = index(line, "=")
  if (i == 0 || cur != want_s) next
  if (trim(substr(line, 1, i - 1)) == want_k) { val = trim(substr(line, i + 1)); found = 1 }
}
END { if (found) print val; exit found ? 0 : 1 }
' "$file"
