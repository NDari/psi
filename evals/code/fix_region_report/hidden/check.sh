. /evallib/check.sh
check_output top3 /hidden/want_top3.txt python3 report.py /hidden/sales.csv --top 3
check_output full /hidden/want_full.txt python3 report.py /hidden/sales.csv
check_output ties /hidden/want_ties.txt python3 report.py /hidden/ties.csv
check_output ties_top2 /hidden/want_ties_top2.txt python3 report.py /hidden/ties.csv --top 2
check_output refunds /hidden/want_refunds.txt python3 report.py /hidden/refunds.csv
check_output top_more_than_regions /hidden/want_refunds.txt python3 report.py /hidden/refunds.csv --top 10
