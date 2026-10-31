Since the clocks went back in Germany on October 25, our reminders are wrong. Try:

    python3 -m reminders list --at 09:00 --tz Europe/Berlin --from 2026-10-23T12:00 --count 4

From October 25 on it lists the reminder at 08:00 instead of 09:00. The countdown in brackets is off too: across the night of the clock change it should count the real time that passes (that night has 25 hours). The same goes for the spring change and for other zones.

Please fix reminders/schedule.py (README.md describes next_daily, upcoming, hours_between and format_countdown). Keep the function signatures and the output format.
