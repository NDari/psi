`archive_logs.sh` is unreliable: it archives the wrong files, chokes on spaces and ignores its options. Please fix it so that `bash archive_logs.sh [-n] [-d DAYS] SRC DEST` does this:

- It handles the files directly in SRC (not in subdirectories) whose names end in `.log` and that were last modified more than DAYS days ago (DAYS defaults to 7), in byte order of their names.
- Each such file F is gzipped to `DEST/F.gz` and removed from SRC, printing `archived: F` on stdout. DEST (and its parents) is created if missing.
- If `DEST/F.gz` already exists, leave both alone, print `skip: F` on stderr, carry on with the other files, and exit 1 at the end. Otherwise exit 0.
- `-n` is a dry run: print `would archive: F` for each file instead, and change nothing (not even creating DEST).
- Usage errors (unknown option, missing SRC or DEST, SRC not a directory, DAYS not a non-negative integer): message on stderr, exit 2, change nothing.
- File names may contain spaces.
