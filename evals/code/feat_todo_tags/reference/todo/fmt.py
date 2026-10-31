"""How tasks look on screen."""
from .dates import is_overdue


def format_task(task):
    """One line, for example:  3 [ ] (p1) Pay rent  due 2026-11-01 ! #home

    The "!" marks an overdue open task. Tags come last.
    """
    mark = "x" if task.done else " "
    line = f"{task.id:>3} [{mark}] (p{task.priority}) {task.title}"
    if task.due:
        line += f"  due {task.due}"
        if not task.done and is_overdue(task.due):
            line += " !"
    for tag in task.tags:
        line += f" #{tag}"
    return line


def format_list(tasks):
    if not tasks:
        return "no tasks\n"
    return "".join(format_task(t) + "\n" for t in tasks)


SORT_KEYS = {
    "id": lambda t: t.id,
    "priority": lambda t: (t.priority, t.id),
    "due": lambda t: (t.due is None, t.due or "", t.id),
}
