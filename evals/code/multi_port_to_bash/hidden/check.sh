. /evallib/check.sh
check_output one_file /hidden/want1.out bash tally.sh /hidden/a.log
check_output two_files /hidden/want2.out bash tally.sh /hidden/a.log /hidden/b.log
check_output level_filter /hidden/want3.out bash tally.sh -l WARN /hidden/a.log /hidden/b.log
check_output ties_by_name /hidden/want4.out bash tally.sh /hidden/ties.log
check_output few_components /hidden/want5.out bash tally.sh /hidden/one.log
check_output level_no_match /hidden/want6.out bash tally.sh -l INFO /hidden/one.log /hidden/empty.log
check_output no_trailing_newline /hidden/want7.out bash tally.sh /hidden/b.log /hidden/a.log
check usage_errors bash -c '
for args in "" "-l" "-l WARN"; do
  out=$(bash tally.sh $args 2>/tmp/usage.err); s=$?
  [ "$s" = 2 ] && [ -z "$out" ] && [ "$(cat /tmp/usage.err)" = "usage: tally [-l LEVEL] FILE..." ] || { echo "args [$args]: status $s, stdout [$out], stderr [$(cat /tmp/usage.err)]"; exit 1; }
done'
check missing_file bash -c '
out=$(bash tally.sh /hidden/a.log /hidden/nope.log 2>/tmp/missing.err); s=$?
[ "$s" = 1 ] && [ -z "$out" ] && [ "$(cat /tmp/missing.err)" = "tally: cannot read /hidden/nope.log" ] || { echo "status $s, stdout [$out], stderr [$(cat /tmp/missing.err)]"; exit 1; }'
check no_python bash -c 'test -f tally.sh && ! grep -qi python tally.sh'
