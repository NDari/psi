`rotate_backups.sh` is deleting the wrong backups, please fix it. It's run as `bash rotate_backups.sh DIR KEEP` and should:

- treat as backups only the files in DIR named exactly `backup-YYYY-MM-DD.tar.gz`; leave every other file alone;
- keep the KEEP newest backups, judged by the date in the name (not the file modification time), and delete the rest;
- print `deleted: NAME` for each deleted backup, oldest first;
- work when DIR contains spaces;
- exit 2 with a usage message on stderr, deleting nothing, if KEEP is not a positive integer or DIR is not a directory.
