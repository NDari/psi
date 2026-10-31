. /evallib/check.sh
for s in add_list filter tag_untag errors tags_cmd old_file; do
  check_output "$s" /hidden/want_$s.txt bash /hidden/s_$s.sh
done
check_output existing_commands_unchanged /hidden/want_existing.txt bash /hidden/s_existing.sh
check visible_tests python3 -m unittest -q
