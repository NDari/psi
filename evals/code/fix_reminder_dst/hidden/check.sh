. /evallib/check.sh
unittests /hidden/test_hidden_reminders.py
check_output cli_berlin_fall /hidden/want_berlin.txt python3 -m reminders list --at 09:00 --tz Europe/Berlin --from 2026-10-23T12:00 --count 4
check_output cli_sydney_spring /hidden/want_sydney.txt python3 -m reminders list --at 20:45 --tz Australia/Sydney --from 2026-10-02T21:00 --count 3
