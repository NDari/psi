Please add tags to the todo tool (package todo/, run as `python3 -m todo`):

- `add TITLE --tag NAME` (the option can be repeated), plus two new commands: `tag ID NAME...` adds tags to a task (tags it already has are left alone) and prints `tagged ID`; `untag ID NAME...` removes tags (ones it does not have are ignored) and prints `untagged ID`.
- Tag names are case-insensitive and stored in lower case. After lowercasing, a valid tag matches `[a-z0-9][a-z0-9-]*`; anything else is the error `bad tag: NAME` (NAME as typed), reported like the existing errors (stderr `todo: ...`, exit status 1), and nothing is saved. A task's tags keep the order in which they were first added, without duplicates.
- `list --tag NAME` (repeatable) shows only tasks that have all the given tags, together with the existing --all and --sort.
- In list output a task's tags are appended at the very end of its line, each as a space and `#name`: `  1 [ ] (p1) Pay rent  due 2026-10-30 #home #money`.
- New command `tags`: one line `NAME COUNT` per tag, sorted by name, counting all tasks that have it (done ones too); no output when there are no tags.
- Tags are saved in the file as a "tags" list on each task. Files written before this change, without "tags", must still load.
