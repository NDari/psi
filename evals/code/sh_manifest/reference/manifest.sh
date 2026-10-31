#!/usr/bin/env bash
# manifest.sh create DIR | verify DIR MANIFEST
export LC_ALL=C

usage() {
  echo "usage: manifest.sh create DIR | manifest.sh verify DIR MANIFEST" >&2
  exit 2
}

create() { # DIR: print the manifest of DIR
  (cd -- "$1" && find . -type f -print0) | sort -z | while IFS= read -r -d '' p; do
    p=${p#./}
    sum=$(sha256sum < "$1/$p") || exit 2
    size=$(stat -c %s -- "$1/$p")
    printf '%s %s %s\n' "${sum%% *}" "$size" "$p"
  done
}

case ${1-} in
create)
  [ $# -eq 2 ] && [ -d "$2" ] || usage
  create "$2"
  ;;
verify)
  [ $# -eq 3 ] && [ -d "$2" ] && [ -f "$3" ] && [ -r "$3" ] || usage
  tmp=$(mktemp)
  trap 'rm -f "$tmp"' EXIT
  create "$2" > "$tmp"
  awk '
    { sum = $1; size = $2; p = $0; sub(/^[^ ]+ [^ ]+ /, "", p) }
    FNR == NR { want[p] = sum " " size; next }
    { have[p] = sum " " size }
    END {
      for (p in want)
        if (!(p in have)) print p "\tMISSING"
        else if (want[p] != have[p]) print p "\tCHANGED"
      for (p in have) if (!(p in want)) print p "\tEXTRA"
    }' "$3" "$tmp" | sort -t "$(printf '\t')" -k1,1 | awk -F '\t' '{ print $2 " " $1; n++ } END { exit n > 0 }'
  ;;
*)
  usage
  ;;
esac
