. /evallib/check.sh
check_output report_all /hidden/want_all.txt python3 -m latency report /hidden/gw.log
check_output report_endpoint /hidden/want_search.txt python3 -m latency report /hidden/gw.log --endpoint /api/search
check_output report_by_endpoint /hidden/want_by.txt python3 -m latency report /hidden/gw.log --by-endpoint
unittests /hidden/test_hidden_latency.py
