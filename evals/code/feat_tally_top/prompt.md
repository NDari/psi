tally.py needs two new options:

- `--top N`: print only the first N lines of the usual output. N must be a positive integer; for 0, a negative number or a non-number, tally.py must fail with an argparse usage error (exit status 2) and print nothing on stdout.
- `--min-length L`: ignore words shorter than L characters (length counted after the existing apostrophe stripping). L is a non-negative integer, default 0.

They combine with each other and with `--ignore-case`; `--top` applies after `--min-length` filtering. Keep the output format and ordering as they are.
