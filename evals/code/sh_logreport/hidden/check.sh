. /evallib/check.sh
w=$(mktemp -d)
bash logreport.sh /hidden/big.log > "$w/big" 2>/dev/null
section() { # FILE HEADER: lines after HEADER up to the next unindented line
  awk -v h="$2" '$0 == h {on = 1; next} on && /^[^ ]/ {on = 0} on' "$1"
}
section /hidden/want_big.txt status: > "$w/want_status"
section /hidden/want_big.txt 'top paths:' > "$w/want_top"
check_output full_report /hidden/want_big.txt cat "$w/big"
check requests_and_ips bash -c 'grep -qx "requests: 60" "$1" && grep -qx "unique ips: 9" "$1"' _ "$w/big"
check bytes_dash_as_zero bash -c 'bash logreport.sh /hidden/tie.log | grep -qx "bytes: 150"'
check_output status_section "$w/want_status" bash -c "$(declare -f section); section '$w/big' status:"
check_output top_paths_without_query "$w/want_top" bash -c "$(declare -f section); section '$w/big' 'top paths:'"
check_output top_paths_ties /hidden/want_tie.txt bash logreport.sh /hidden/tie.log
check_output fewer_paths_and_blank_lines /hidden/want_two.txt bash logreport.sh /hidden/two.log
rm -rf "$w"
