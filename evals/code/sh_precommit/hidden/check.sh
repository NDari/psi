. /evallib/check.sh
w=$(mktemp -d)
export HOOK=$PWD/hooks/pre-commit HOME=$w GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=Dev GIT_AUTHOR_EMAIL=dev@example.com GIT_COMMITTER_NAME=Dev GIT_COMMITTER_EMAIL=dev@example.com
# case.sh DIR SETUP [MESSAGE...]: in a fresh repo with the hook installed, run
# SETUP, then commit. With no MESSAGE the commit must succeed; otherwise it
# must fail and the "pre-commit: " lines on stderr must be exactly the MESSAGEs.
cat > "$w/case.sh" <<'CASE'
dir=$1 setup=$2
shift 2
mkdir "$dir" && cd "$dir" || exit 9
git init -q -b main .
mkdir -p app config
printf 'def f():\n    # call breakpoint() here when debugging\n    return 1\n' > app/main.py
printf 'SECRET=old\n' > config/.env
git add -A && git commit -q --no-verify -m init || exit 9
cp "$HOOK" .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit
eval "$setup" >/dev/null 2>&1 || { echo "setup failed"; exit 9; }
git commit -q -m change 2> ../err.$$
status=$?
if [ $# -eq 0 ]; then
  [ $status -eq 0 ] || { echo "commit rejected:"; cat ../err.$$; exit 1; }
  exit 0
fi
[ $status -ne 0 ] || { echo "commit accepted"; exit 1; }
printf '%s\n' "$@" | LC_ALL=C sort > ../want.$$
grep '^pre-commit: ' ../err.$$ | LC_ALL=C sort > ../got.$$
diff ../want.$$ ../got.$$
CASE
n=0
case_() { # NAME SETUP [MESSAGE...]
  local name=$1
  shift
  n=$((n + 1))
  check "$name" bash "$w/case.sh" "$w/r$n" "$@"
}
case_ clean_commit_allowed 'printf "def g():\n    return 2\n" > app/util.py && git add app/util.py'
case_ env_at_root_rejected 'printf "K=v\n" > .env && git add .env' 'pre-commit: .env file staged: .env'
case_ nested_env_rejected 'printf "SECRET=new\n" > config/.env && git add config/.env' 'pre-commit: .env file staged: config/.env'
case_ env_example_allowed 'printf "K=\n" > .env.example && printf "K=\n" > config/.env.local && head -c 600000 /dev/zero > big.bin && git add -A' 'pre-commit: big.bin is larger than 500 KB'
case_ breakpoint_rejected 'printf "def f():\n    breakpoint()\n    return 1\n" > app/main.py && git add app/main.py' 'pre-commit: breakpoint() in app/main.py'
case_ breakpoint_only_in_worktree 'printf "def h():\n    return 3\n" > app/h.py && printf "A=1\n" > .env && git add app/h.py .env && printf "def h():\n    breakpoint()\n    return 3\n" > app/h.py' 'pre-commit: .env file staged: .env'
case_ only_added_lines_count 'printf "def f():\n    # call breakpoint() here when debugging\n    return 2\n" > app/main.py && printf "def k():\n    breakpoint()\n" > app/k.py && git add app/main.py app/k.py' 'pre-commit: breakpoint() in app/k.py'
case_ breakpoint_outside_py_allowed 'printf "use breakpoint() to debug\n" > NOTES.txt && printf "A=1\n" > app/.env && git add NOTES.txt app/.env' 'pre-commit: .env file staged: app/.env'
case_ large_file_rejected 'head -c 600000 /dev/zero > "big file.bin" && head -c 512000 /dev/zero > ok.bin && git add -A' 'pre-commit: big file.bin is larger than 500 KB'
case_ deleting_env_allowed 'git rm -q config/.env && printf "breakpoint()\n" > app/x.py && git add app/x.py' 'pre-commit: breakpoint() in app/x.py'
case_ all_problems_reported 'printf "breakpoint()\n" > "app/my tool.py" && printf "A=1\n" > app/.env && head -c 512001 /dev/zero > data.bin && git add -A' 'pre-commit: breakpoint() in app/my tool.py' 'pre-commit: .env file staged: app/.env' 'pre-commit: data.bin is larger than 500 KB'
cd /
rm -rf "$w"
