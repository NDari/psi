. /evallib/check.sh
H=/hidden
check_output default_unchanged $H/want_default.txt python3 tally.py $H/a.txt
check_output top3 $H/want_top3.txt python3 tally.py --top 3 $H/a.txt
check_output top_more_than_words $H/want_top50.txt python3 tally.py --top 50 $H/a.txt $H/b.txt
check_output min_length4 $H/want_min4.txt python3 tally.py --min-length 4 $H/a.txt
check_output combined_with_ignore_case $H/want_combo.txt python3 tally.py -i --min-length 3 --top 4 $H/a.txt $H/b.txt
check_output top_on_stdin $H/want_stdin.txt sh -c "python3 tally.py --top 2 < $H/b.txt"
check top_invalid_rejected bash $H/rejects.sh
