# Sourced by the scenario scripts: a fresh todo file and a fixed "today".
d=$(mktemp -d)
trap 'rm -rf "$d"' EXIT
export TODO_FILE=$d/todo.json TODO_TODAY=2026-10-28
t() {
  echo "\$ todo $*"
  python3 -m todo "$@" 2>&1
  echo "[exit $?]"
}
