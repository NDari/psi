"""Run jobs when their schedules say so.

    runner = Runner(clock=datetime.now)
    runner.add("backup", parse_schedule("daily 02:00"), do_backup)
    runner.tick()   # call regularly; runs every job that is due
"""
from datetime import datetime

from schedule import parse_schedule


class Job:
    def __init__(self, name, schedule, action, now):
        self.name = name
        self.schedule = schedule
        self.action = action
        self.next_run = schedule.next_after(now)
        self.runs = 0

    def __repr__(self):
        return f"Job({self.name!r}, {self.schedule!r}, next={self.next_run:%Y-%m-%d %H:%M})"


class Runner:
    def __init__(self, clock=datetime.now):
        self.clock = clock
        self.jobs = {}

    def add(self, name, schedule, action):
        if name in self.jobs:
            raise ValueError(f"duplicate job {name!r}")
        self.jobs[name] = Job(name, schedule, action, self.clock())

    def tick(self):
        """Run each due job once (missed runs are not caught up). Returns the names run."""
        now = self.clock()
        ran = []
        for job in self.jobs.values():
            if job.next_run <= now:
                job.action()
                job.runs += 1
                job.next_run = job.schedule.next_after(now)
                ran.append(job.name)
        return ran


def load_jobs(path):
    """Read jobs.conf lines "NAME: SCHEDULE" into (name, schedule) pairs."""
    jobs = []
    with open(path, encoding="utf-8") as f:
        for number, line in enumerate(f, 1):
            line = line.split("#", 1)[0].strip()
            if not line:
                continue
            name, sep, sched = line.partition(":")
            if not sep:
                raise ValueError(f"{path}:{number}: expected NAME: SCHEDULE")
            try:
                jobs.append((name.strip(), parse_schedule(sched)))
            except ValueError as e:
                raise ValueError(f"{path}:{number}: {e}") from None
    return jobs
