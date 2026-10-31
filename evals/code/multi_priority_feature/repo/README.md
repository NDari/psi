# tracker

A small task tracker for the terminal. Tasks live in one JSON file
(`$TRACKER_FILE`, or `~/.tracker.json`; `--file PATH` overrides both).
Python 3.11 or later, standard library only.

## Usage

    python3 -m tracker add Buy milk -p home -t shop --due tomorrow
    python3 -m tracker list
    python3 -m tracker list --status all --sort due
    python3 -m tracker done 3
    python3 -m tracker report

`--today YYYY-MM-DD` before the command pretends today is that date, which
the tests use.

### Commands

| command | what it does |
|---|---|
| `add TITLE... [-p PROJECT] [-t TAG]... [--due DATE] [--notes TEXT]` | add a task |
| `list [--status open\|done\|all] [-p PROJECT] [-t TAG] [--overdue] [--search TEXT] [--sort KEY] [--reverse]` | list tasks |
| `done ID...` | mark tasks done |
| `undo ID` | reopen a done task |
| `edit ID [--title T] [-p P \| --no-project] [--due D \| --no-due] [--add-tag T] [--remove-tag T] [--notes N]` | change a task |
| `remove ID...` | delete tasks |
| `show ID` | every field of one task |
| `projects` | open and done counts per project |
| `tags` | open task counts per tag |
| `report` | a summary |

Sort keys: `id`, `due`, `title`, `created`, `project`. Tasks without the
sort value come last; ties are broken by id.

Dates: `YYYY-MM-DD`, `today`, `tomorrow`, `yesterday`, `+3d`, `-1w`, a
weekday name (`fri`, `friday`: the next one after today), `eow` (the coming
Sunday).

### List lines

    ID [ ] TITLE (PROJECT) due YYYY-MM-DD #tag #tag

`[x]` marks a done task and `!` after the date an overdue one.

## Layout

- `tracker/model.py`: the `Task` dataclass and its validation
- `tracker/storage.py`: loading and saving the JSON file
- `tracker/dates.py`: date parsing relative to today
- `tracker/filters.py`: filtering, sorting and grouping
- `tracker/formatting.py`: list lines, the `show` view, tables
- `tracker/report.py`: the `report` summary
- `tracker/cli.py`: the argparse front end

## File format

    {"version": 2, "tasks": [{"id": 1, "title": "...", "project": null,
      "tags": [], "status": "open", "created": "2026-01-02", "due": null,
      "done_on": null, "notes": ""}]}

Version 1 files (a bare list of tasks) still load. Unknown keys are ignored
and optional keys may be missing.

## Tests

    python3 -m unittest
