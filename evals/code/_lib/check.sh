# Helpers for a task's hidden/check.sh (eval/code/README.md). Source it:
#   . /evallib/check.sh
# Each helper prints one line, "PASS <name>" or "FAIL <name>: <why>", and
# never exits, so every check runs. The working directory is the graded
# project; hidden files are under /hidden.

# check NAME CMD...: passes when CMD exits 0 within 60 seconds.
check() {
  local name=$1 out
  shift
  if out=$(timeout 60 "$@" 2>&1); then
    echo "PASS $name"
  else
    echo "FAIL $name: exit $?: $(printf '%s' "$out" | tail -n 3 | tr '\n' ' ' | cut -c1-200)"
  fi
}

# check_output NAME WANT_FILE CMD...: passes when CMD's stdout equals the
# contents of WANT_FILE byte for byte (and CMD exits 0).
check_output() {
  local name=$1 want=$2 got
  shift 2
  local code=0
  got=$(mktemp)
  timeout 60 "$@" >"$got" 2>/dev/null || code=$?
  if ((code != 0)); then
    echo "FAIL $name: exit $code"
  elif cmp -s "$got" "$want"; then
    echo "PASS $name"
  else
    echo "FAIL $name: output differs: $(diff "$want" "$got" | head -n 4 | tr '\n' ' ' | cut -c1-200)"
  fi
  rm -f "$got"
}

# unittests FILE...: run hidden unittest files, one line per test.
unittests() {
  timeout 120 python3 /evallib/unittest_check.py "$@" 2>/dev/null || true
}
