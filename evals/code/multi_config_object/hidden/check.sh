. /evallib/check.sh
unittests /hidden/test_config.py
check_output cli_with_config /hidden/custom.out python3 -m logsift /hidden/custom.log --config /hidden/custom.json
check_output cli_default /hidden/default.out python3 -m logsift /hidden/default.log
