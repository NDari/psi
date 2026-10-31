. /evallib/check.sh
check_output default_top10 /hidden/want_default.txt bash wordfreq.sh /hidden/text.txt
check_output option_n /hidden/want_n4.txt bash wordfreq.sh -n 4 /hidden/text.txt
check_output fewer_than_n /hidden/want_small.txt bash wordfreq.sh -n 5 /hidden/small.txt
check_output reads_stdin /hidden/want_n4.txt bash -c 'bash wordfreq.sh -n 4 < /hidden/text.txt'
check_output apostrophe_splits <(printf '3 t\n2 don\n1 won\n') bash -c "printf \"Don't don't won't\\n\" | bash wordfreq.sh -n 3"
check_output case_folded <(printf '3 apple\n') bash -c 'printf "Apple APPLE apple\n" | bash wordfreq.sh'
