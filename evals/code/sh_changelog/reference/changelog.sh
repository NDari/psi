#!/usr/bin/env bash
# changelog.sh FROM [TO]: release notes for the commits in FROM..TO.
if [ $# -lt 1 ] || [ $# -gt 2 ]; then
  echo "usage: changelog.sh FROM [TO]" >&2
  exit 1
fi
from=$1 to=${2:-HEAD}
for rev in "$from" "$to"; do
  if ! git rev-parse --verify --quiet "$rev^{commit}" >/dev/null; then
    echo "changelog.sh: not a valid revision: $rev" >&2
    exit 1
  fi
done
git log --no-merges --reverse --format='%H %s' "$from..$to" | awk '
{
  hash = substr($1, 1, 7)
  subject = substr($0, length($1) + 2)
  sect = "o"; desc = subject
  if (match(subject, /^(feat|fix)(\([^)]*\))?: /)) {
    head = substr(subject, 1, RLENGTH - 2)
    desc = substr(subject, RLENGTH + 1)
    sect = (substr(head, 1, 4) == "feat") ? "f" : "x"
    p = index(head, "(")
    if (p) desc = substr(head, p + 1, length(head) - p - 1) ": " desc
  }
  lines[sect] = lines[sect] "- " desc " (" hash ")\n"
}
END {
  title["f"] = "Features"; title["x"] = "Fixes"; title["o"] = "Other"
  split("f x o", order, " ")
  sep = ""
  for (i = 1; i <= 3; i++) {
    s = order[i]
    if (s in lines) { printf "%s## %s\n%s", sep, title[s], lines[s]; sep = "\n" }
  }
}'
