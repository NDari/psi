# jobrunner

A small in-process job runner. Jobs are listed in jobs.conf:

    # name: schedule
    backup: daily 02:00
    sync: every 15m

`schedule.py` holds the schedule types and `parse_schedule`; `runner.py`
runs due jobs. Tests: `python3 -m unittest`.
