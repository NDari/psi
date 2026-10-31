. /evallib/check.sh
H=/hidden
unittests $H/test_jsonl.py
check_output clf_report_unchanged $H/want_clf.txt python3 -m logstats $H/access.log
check_output format_jsonl $H/want_jsonl.txt python3 -m logstats --format jsonl $H/proxy.log
check_output auto_detects_jsonl $H/want_jsonl.txt python3 -m logstats $H/proxy.log
check_output auto_mixed_files $H/want_mixed.txt python3 -m logstats --top 3 $H/access.log $H/proxy.log
check_output forced_clf_skips_json $H/want_forced_clf.txt python3 -m logstats --format clf $H/proxy.log
check_output jsonl_since_utc $H/want_since.txt python3 -m logstats --since 2026-10-10T14:00:00+00:00 --top 2 $H/proxy.log
check visible_tests python3 -m unittest -q
