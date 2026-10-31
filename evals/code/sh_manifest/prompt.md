Please write `manifest.sh` to record and check the contents of a directory tree. It has two modes:

- `bash manifest.sh create DIR` prints one line per regular file under DIR (recursively, hidden files included; directories and symlinks are not listed): `SHA256 SIZE PATH`, separated by single spaces, where SIZE is in bytes and PATH is relative to DIR (no leading `./`). Lines are sorted by PATH in byte order (as `LC_ALL=C sort` would).
- `bash manifest.sh verify DIR MANIFEST` compares DIR with a manifest made by `create`. It prints `MISSING PATH` for files in the manifest but not in DIR, `CHANGED PATH` for files whose hash or size differs, and `EXTRA PATH` for files in DIR not in the manifest, one line each, sorted by PATH in byte order. It exits 0 if there are no differences and 1 otherwise.
- Any usage error (unknown mode, wrong number of arguments, DIR not a directory, MANIFEST not readable) prints a message to stderr and exits 2.

File names may contain spaces (but never newlines).
