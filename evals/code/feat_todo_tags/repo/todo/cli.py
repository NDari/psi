"""Command line interface.

    python3 -m todo add "Pay rent" -p 1 --due fri
    python3 -m todo list [--all] [--sort id|priority|due]
    python3 -m todo done 3 [4 ...]
    python3 -m todo edit 3 --title "Pay the rent" --due 2026-11-01
    python3 -m todo rm 3

The file is --file PATH, else $TODO_FILE, else ./todo.json. Errors are
printed as "todo: MESSAGE" on stderr with exit status 1.
"""
import argparse
import sys

from . import __version__
from .dates import parse_due, today
from .fmt import SORT_KEYS, format_list
from .model import Task, TodoError
from .store import Store, default_path, find, next_id


def cmd_add(store, args):
    tasks = store.load()
    task = Task(
        id=next_id(tasks),
        title=args.title,
        priority=args.priority,
        created=today().isoformat(),
        due=parse_due(args.due) if args.due else None,
    )
    tasks.append(task)
    store.save(tasks)
    print(f"added {task.id}")


def cmd_list(store, args):
    tasks = store.load()
    if not args.all:
        tasks = [t for t in tasks if not t.done]
    tasks.sort(key=SORT_KEYS[args.sort])
    sys.stdout.write(format_list(tasks))


def cmd_done(store, args):
    tasks = store.load()
    for task_id in args.ids:
        find(tasks, task_id).done = True
    store.save(tasks)
    for task_id in args.ids:
        print(f"done {task_id}")


def cmd_edit(store, args):
    tasks = store.load()
    task = find(tasks, args.id)
    if args.title is not None:
        task.title = args.title
    if args.priority is not None:
        task.priority = args.priority
    if args.due is not None:
        task.due = None if args.due == "none" else parse_due(args.due)
    Task(**vars(task))  # validate the edited values
    store.save(tasks)
    print(f"edited {task.id}")


def cmd_rm(store, args):
    tasks = store.load()
    task = find(tasks, args.id)
    tasks.remove(task)
    store.save(tasks)
    print(f"removed {task.id}")


def build_parser():
    p = argparse.ArgumentParser(prog="todo", description="A task list in a JSON file.")
    p.add_argument("--file", default=None, help="the todo file (default $TODO_FILE or ./todo.json)")
    p.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    sub = p.add_subparsers(dest="command", required=True)

    a = sub.add_parser("add", help="add a task")
    a.add_argument("title")
    a.add_argument("-p", "--priority", type=int, default=2, choices=(1, 2, 3))
    a.add_argument("--due", help="today, tomorrow, a weekday or YYYY-MM-DD")
    a.set_defaults(run=cmd_add)

    l = sub.add_parser("list", help="list open tasks")
    l.add_argument("--all", action="store_true", help="include done tasks")
    l.add_argument("--sort", choices=sorted(SORT_KEYS), default="id")
    l.set_defaults(run=cmd_list)

    d = sub.add_parser("done", help="mark tasks done")
    d.add_argument("ids", type=int, nargs="+", metavar="ID")
    d.set_defaults(run=cmd_done)

    e = sub.add_parser("edit", help="change a task")
    e.add_argument("id", type=int)
    e.add_argument("--title")
    e.add_argument("-p", "--priority", type=int, choices=(1, 2, 3))
    e.add_argument("--due", help='a date as for add, or "none" to clear it')
    e.set_defaults(run=cmd_edit)

    r = sub.add_parser("rm", help="delete a task")
    r.add_argument("id", type=int)
    r.set_defaults(run=cmd_rm)
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    store = Store(args.file or default_path())
    try:
        args.run(store, args)
    except TodoError as e:
        print(f"todo: {e}", file=sys.stderr)
        return 1
    return 0
