. /evallib/check.sh
S=$PWD/archive_logs.sh
w=$(mktemp -d)
mk() { # DIR: the fixture
  mkdir -p "$1/sub"
  local f
  for f in old.log "app server.log" app.log recent.log notes.txt sub/inner.log three.log; do
    printf 'contents of %s\n' "$f" > "$1/$f"
  done
  touch -d '30 days ago' "$1/old.log" "$1/app server.log" "$1/app.log" "$1/notes.txt" "$1/sub/inner.log"
  touch -d '1 day ago' "$1/recent.log"
  touch -d '3 days ago' "$1/three.log"
}
mk "$w/a"
(cd "$w" && bash "$S" "$w/a" "$w/out/2026/oct" > "$w/a.out" 2> "$w/a.err"; echo $? > "$w/a.status")
gz() { [ -f "$1/$2.gz" ] && [ "$(gzip -dc "$1/$2.gz")" = "contents of $2" ]; }
export -f gz
check old_log_archived bash -c 'gz "$1/out/2026/oct" old.log && [ ! -e "$1/a/old.log" ] && [ "$(cat "$1/a.status")" = 0 ]' _ "$w"
check spaces_in_names bash -c 'gz "$1/out/2026/oct" "app server.log" && [ ! -e "$1/a/app server.log" ]' _ "$w"
check recent_and_other_files_kept bash -c 'cd "$1/a" && [ -f recent.log ] && [ -f three.log ] && [ -f notes.txt ] && [ -f sub/inner.log ] && [ "$(ls "$1/out/2026/oct" | LC_ALL=C sort | tr "\n" "|")" = "app server.log.gz|app.log.gz|old.log.gz|" ]' _ "$w"
check_output stdout_in_name_order <(printf 'archived: %s\n' "app server.log" app.log old.log) cat "$w/a.out"
mk "$w/b"
check days_option bash -c 'cd "$2" && bash "$1" -d 2 "$2/b" "$2/bout" >/dev/null && gz "$2/bout" three.log && gz "$2/bout" old.log && [ -f "$2/b/recent.log" ] && [ ! -e "$2/bout/recent.log.gz" ]' _ "$S" "$w"
mk "$w/c"
check_output dry_run_output <(printf 'would archive: %s\n' "app server.log" app.log old.log) bash -c 'cd "$2" && bash "$1" -n "$2/c" "$2/cout"' _ "$S" "$w"
check dry_run_changes_nothing bash -c 'cd "$1/c" && [ ! -e "$1/cout" ] && [ "$(find . -type f | LC_ALL=C sort | tr "\n" "|")" = "./app server.log|./app.log|./notes.txt|./old.log|./recent.log|./sub/inner.log|./three.log|" ]' _ "$w"
mk "$w/d"
mkdir -p "$w/dout"
printf 'already here\n' | gzip > "$w/dout/app.log.gz"
(cd "$w" && bash "$S" "$w/d" "$w/dout" > "$w/d.out" 2> "$w/d.err"; echo $? > "$w/d.status")
check existing_gz_skipped bash -c '[ "$(cat "$1/d.status")" = 1 ] && grep -qx "skip: app.log" "$1/d.err" && [ "$(gzip -dc "$1/dout/app.log.gz")" = "already here" ] && [ -f "$1/d/app.log" ] && gz "$1/dout" old.log && gz "$1/dout" "app server.log" && ! grep -q "archived: app.log" "$1/d.out"' _ "$w"
mk "$w/e"
check usage_errors_exit_2 bash -c 'cd "$2"; for a in "-x e eout" "e" "nope eout" "-d abc e eout" "-d -1 e eout"; do e=$(bash "$1" $a 2>&1 >/dev/null); r=$?; [ $r -eq 2 ] && [ -n "$e" ] || { echo "args $a: exit $r"; exit 1; }; done; [ ! -e eout ] && [ "$(find e -type f -name "*.log" | wc -l)" = 6 ]' _ "$S" "$w"
cd /
rm -rf "$w"
