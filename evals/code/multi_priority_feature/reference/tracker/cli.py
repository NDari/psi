"""The command line: python3 -m tracker [--file PATH] [--today DATE] COMMAND ...

Commands:
  add TITLE...        add a task
  list                list tasks (open ones by default)
  done ID...          mark tasks done
  undo ID             reopen a done task
  edit ID             change a task's fields
  remove ID...        delete tasks
  show ID             print every field of one task
  projects            open and done counts per project
  tags                open task counts per tag
  report              a summary of everything

Exit status: 0 on success, 1 on an error (printed to stderr as
"error: ..."), 2 on a usage error.
"""

from __future__ import annotations

import argparse
import sys
from datetime import date

from . import storage
from .dates import parse_date
from .filters import SORT_KEYS, all_tags, filter_tasks, group_by_project, sort_tasks
from .formatting import format_detail, format_line, format_table
from .model import PRIORITIES, Task
from .report import build_report


class CommandError(Exception):
    """A user-facing error; main prints it and exits with status 1."""


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="tracker", description="A small task tracker.")
    p.add_argument("--file", help="task file (default: $TRACKER_FILE or ~/.tracker.json)")
    p.add_argument("--today", help="pretend today is this date (YYYY-MM-DD)")
    sub = p.add_subparsers(dest="command", required=True, metavar="COMMAND")

    a = sub.add_parser("add", help="add a task")
    a.add_argument("title", nargs="+", help="the task title")
    a.add_argument("-p", "--project", help="project name")
    a.add_argument("-t", "--tag", action="append", default=[], help="a tag (repeatable)")
    a.add_argument("--due", help="due date: YYYY-MM-DD, today, tomorrow, +3d, fri, ...")
    a.add_argument("--notes", default="", help="free text")
    a.add_argument("--priority", choices=PRIORITIES, default="normal")

    ls = sub.add_parser("list", help="list tasks")
    ls.add_argument("--status", choices=("open", "done", "all"), default="open")
    ls.add_argument("-p", "--project", help="only this project")
    ls.add_argument("-t", "--tag", help="only tasks with this tag")
    ls.add_argument("--overdue", action="store_true", help="only overdue tasks")
    ls.add_argument("--search", help="text in the title or notes")
    ls.add_argument("--priority", choices=PRIORITIES, help="only this priority")
    ls.add_argument("--sort", choices=SORT_KEYS, default="id")
    ls.add_argument("--reverse", action="store_true")

    d = sub.add_parser("done", help="mark tasks done")
    d.add_argument("ids", nargs="+", type=int)

    u = sub.add_parser("undo", help="reopen a done task")
    u.add_argument("id", type=int)

    e = sub.add_parser("edit", help="change a task")
    e.add_argument("id", type=int)
    e.add_argument("--title")
    e.add_argument("-p", "--project")
    e.add_argument("--no-project", action="store_true", help="remove the project")
    e.add_argument("--due")
    e.add_argument("--no-due", action="store_true", help="remove the due date")
    e.add_argument("--add-tag", action="append", default=[])
    e.add_argument("--remove-tag", action="append", default=[])
    e.add_argument("--notes")

    r = sub.add_parser("remove", help="delete tasks")
    r.add_argument("ids", nargs="+", type=int)

    s = sub.add_parser("show", help="show one task")
    s.add_argument("id", type=int)

    sub.add_parser("projects", help="counts per project")
    sub.add_parser("tags", help="open task counts per tag")
    sub.add_parser("report", help="a summary")
    return p


# Commands. Each takes (args, tasks, today), prints its output, and returns
# True when the task list changed and must be saved.


def cmd_add(args, tasks, today) -> bool:
    title = " ".join(args.title)
    due = _parse_due(args.due, today) if args.due else None
    try:
        task = Task(
            id=storage.next_id(tasks),
            title=title,
            project=args.project,
            tags=args.tag,
            created=today,
            due=due,
            notes=args.notes,
            priority=args.priority,
        )
    except ValueError as e:
        raise CommandError(str(e)) from None
    tasks.append(task)
    print(f"Added task {task.id}: {task.title}")
    return True


def cmd_list(args, tasks, today) -> bool:
    try:
        selected = filter_tasks(
            tasks,
            status=args.status,
            project=args.project,
            tag=args.tag,
            overdue=args.overdue,
            today=today,
            text=args.search,
            priority=args.priority,
        )
    except ValueError as e:
        raise CommandError(str(e)) from None
    selected = sort_tasks(selected, args.sort, reverse=args.reverse)
    if not selected:
        print("No tasks.")
        return False
    for t in selected:
        print(format_line(t, today))
    return False


def cmd_done(args, tasks, today) -> bool:
    targets = [_find(tasks, i) for i in args.ids]
    for t in targets:
        try:
            t.mark_done(today)
        except ValueError as e:
            raise CommandError(str(e)) from None
    for t in targets:
        print(f"Done: {t.id} {t.title}")
    return True


def cmd_undo(args, tasks, today) -> bool:
    t = _find(tasks, args.id)
    try:
        t.reopen()
    except ValueError as e:
        raise CommandError(str(e)) from None
    print(f"Reopened: {t.id} {t.title}")
    return True


def cmd_edit(args, tasks, today) -> bool:
    t = _find(tasks, args.id)
    if args.project and args.no_project:
        raise CommandError("--project and --no-project together")
    if args.due and args.no_due:
        raise CommandError("--due and --no-due together")
    changed = []
    if args.title is not None:
        t.title = args.title
        changed.append("title")
    if args.project:
        t.project = args.project
        changed.append("project")
    if args.no_project:
        t.project = None
        changed.append("project")
    if args.due:
        t.due = _parse_due(args.due, today)
        changed.append("due")
    if args.no_due:
        t.due = None
        changed.append("due")
    if args.add_tag or args.remove_tag:
        tags = [x for x in t.tags if x not in _normalized(args.remove_tag)]
        t.tags = tags + list(args.add_tag)
        changed.append("tags")
    if args.notes is not None:
        t.notes = args.notes
        changed.append("notes")
    if not changed:
        raise CommandError("nothing to change")
    try:
        from .model import validate

        validate(t)
    except ValueError as e:
        raise CommandError(str(e)) from None
    print(f"Updated task {t.id}: {', '.join(dict.fromkeys(changed))}")
    return True


def cmd_remove(args, tasks, today) -> bool:
    targets = [_find(tasks, i) for i in args.ids]
    for t in targets:
        tasks.remove(t)
        print(f"Removed: {t.id} {t.title}")
    return True


def cmd_show(args, tasks, today) -> bool:
    print(format_detail(_find(tasks, args.id), today))
    return False


def cmd_projects(args, tasks, today) -> bool:
    groups = group_by_project(tasks)
    if not groups:
        print("No tasks.")
        return False
    rows = []
    for name, items in groups.items():
        n_open = sum(1 for t in items if t.is_open)
        rows.append((name, n_open, len(items) - n_open))
    print(format_table(("PROJECT", "OPEN", "DONE"), rows))
    return False


def cmd_tags(args, tasks, today) -> bool:
    counts = all_tags(tasks)
    if not counts:
        print("No tags.")
        return False
    print(format_table(("TAG", "OPEN"), [("#" + k, v) for k, v in counts.items()]))
    return False


def cmd_report(args, tasks, today) -> bool:
    print(build_report(tasks, today))
    return False


COMMANDS = {
    "add": cmd_add,
    "list": cmd_list,
    "done": cmd_done,
    "undo": cmd_undo,
    "edit": cmd_edit,
    "remove": cmd_remove,
    "show": cmd_show,
    "projects": cmd_projects,
    "tags": cmd_tags,
    "report": cmd_report,
}


def _find(tasks, task_id: int) -> Task:
    try:
        return storage.find(tasks, task_id)
    except KeyError:
        raise CommandError(f"no task with id {task_id}") from None


def _parse_due(text: str, today: date) -> date:
    try:
        return parse_date(text, today)
    except ValueError as e:
        raise CommandError(str(e)) from None


def _normalized(tags) -> set[str]:
    from .model import normalize_tag

    out = set()
    for tag in tags:
        try:
            out.add(normalize_tag(tag))
        except ValueError as e:
            raise CommandError(str(e)) from None
    return out


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    try:
        today = date.fromisoformat(args.today) if args.today else date.today()
    except ValueError:
        print(f"error: --today: not a date: {args.today!r}", file=sys.stderr)
        return 1
    path = args.file or storage.default_path()
    try:
        tasks = storage.load(path)
        if COMMANDS[args.command](args, tasks, today):
            storage.save(path, tasks)
    except (CommandError, storage.StorageError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0
