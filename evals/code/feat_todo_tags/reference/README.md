# todo

A command line task list kept in a JSON file.

    python3 -m todo add "Pay rent" -p 1 --due fri
    python3 -m todo list
      1 [ ] (p1) Pay rent  due 2026-10-30
    python3 -m todo done 1

Commands: `add`, `list`, `done`, `edit`, `rm`, `tag`, `untag`, `tags`; see `python3 -m todo --help`.

The file is `--file PATH`, else `$TODO_FILE`, else `./todo.json`. Set
`TODO_TODAY=YYYY-MM-DD` to fix the date "today" means (for tests).

Code layout: `model.py` (the Task record and its JSON form), `store.py`
(the file), `dates.py`, `fmt.py` (output) and `cli.py`.

Tests: `python3 -m unittest`.
