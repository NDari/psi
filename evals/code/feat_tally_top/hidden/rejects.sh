# a bad --top value is rejected (exit 2, empty stdout) while a good one works
test "$(python3 tally.py --top 1 /hidden/a.txt | wc -l)" -eq 1 || exit 1
for v in 0 -3 many; do
  out=$(python3 tally.py --top "$v" /hidden/a.txt 2>/dev/null); s=$?
  test "$s" -eq 2 && test -z "$out" || { echo "--top $v: exit $s, stdout ${out:0:40}"; exit 1; }
done
