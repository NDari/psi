. /evallib/check.sh
S=$PWD/rotate_backups.sh
w=$(mktemp -d)
mk() { # DIR: a backup dir whose mtimes run opposite to the dates in the names
  mkdir -p "$1"
  local d
  for d in 2026-10-01 2026-10-05 2026-10-12 2026-09-30 2026-10-20 2025-12-31; do
    echo "$d" > "$1/backup-$d.tar.gz"
  done
  touch -d '2026-10-25 00:00' "$1/backup-2025-12-31.tar.gz"
  touch -d '2026-10-24 00:00' "$1/backup-2026-09-30.tar.gz"
  touch -d '2026-10-23 00:00' "$1/backup-2026-10-01.tar.gz"
  touch -d '2026-10-22 00:00' "$1/backup-2026-10-05.tar.gz"
  touch -d '2026-10-21 00:00' "$1/backup-2026-10-12.tar.gz"
  touch -d '2026-10-01 00:00' "$1/backup-2026-10-20.tar.gz"
  echo notes > "$1/backup-notes.txt"
  echo old > "$1/backup-2020-01-01.tar.gz.part"
  echo other > "$1/other.tar.gz"
  touch -d "2026-09-01 00:00" "$1/backup-notes.txt" "$1/backup-2020-01-01.tar.gz.part"
}
mk "$w/a"
(cd "$w" && bash "$S" "$w/a" 3 > "$w/a.out" 2>/dev/null)
check keeps_newest_by_name bash -c 'cd "$1" && [ -e backup-2026-10-20.tar.gz ] && [ -e backup-2026-10-12.tar.gz ] && [ -e backup-2026-10-05.tar.gz ] && [ ! -e backup-2026-10-01.tar.gz ] && [ ! -e backup-2026-09-30.tar.gz ] && [ ! -e backup-2025-12-31.tar.gz ]' _ "$w/a"
check other_files_untouched bash -c 'cd "$1" && [ -e backup-notes.txt ] && [ -e backup-2020-01-01.tar.gz.part ] && [ -e other.tar.gz ]' _ "$w/a"
check_output deleted_lines_oldest_first <(printf 'deleted: %s\n' backup-2025-12-31.tar.gz backup-2026-09-30.tar.gz backup-2026-10-01.tar.gz) cat "$w/a.out"
mk "$w/my backups"
(cd "$w" && bash "$S" "$w/my backups" 1 >/dev/null 2>&1)
check dir_with_spaces bash -c 'cd "$1" && [ "$(ls backup-*.tar.gz | wc -l)" -eq 1 ] && [ -e backup-2026-10-20.tar.gz ] && [ -e backup-notes.txt ]' _ "$w/my backups"
mk "$w/c"
check keep_more_than_present bash -c 'cd "$2" && bash "$1" "$2/c" 10 && [ "$(ls "$2/c" | wc -l)" -eq 9 ]' _ "$S" "$w"
check bad_keep_exit_2 bash -c 'cd "$2"; for k in 0 abc -1; do e=$(bash "$1" "$2/c" $k 2>&1 >/dev/null); [ $? -eq 2 ] && [ -n "$e" ] || exit 1; done; [ "$(ls "$2/c" | wc -l)" -eq 9 ]' _ "$S" "$w"
check missing_dir_exit_2 bash -c 'cd "$2/c"; e=$(bash "$1" "$2/nope" 1 2>&1 >/dev/null); [ $? -eq 2 ] && [ -n "$e" ] && [ "$(ls "$2/c" | wc -l)" -eq 9 ]' _ "$S" "$w"
rm -rf "$w"
