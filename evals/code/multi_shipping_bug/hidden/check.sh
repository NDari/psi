. /evallib/check.sh
unittests /hidden/test_quotes.py
check_output cli_international_insured /hidden/cli_pl_de.out python3 cli.py PL DE 8 60x40x40 --value 350 --postcode 25938 --account silver --date 2026-12-23
