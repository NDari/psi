. /evallib/check.sh
unittests /hidden/test_convert.py
check_output cli_service /hidden/service.json python3 ini2json.py examples/service.ini
check_output cli_mixed /hidden/mixed.json python3 ini2json.py /hidden/mixed.ini
check cli_error bash -c 'python3 ini2json.py /hidden/bad.ini >/dev/null 2>err.txt; s=$?; m=$(cat err.txt); rm -f err.txt; [ "$s" = 1 ] && [ "$m" = "line 5: cannot parse" ]'
