Write `ini2json.py`, the reverse of json2ini.py. `python3 ini2json.py FILE.ini` prints the JSON exactly as `json.dumps(data, indent=2)` formats it, followed by a newline. It should also expose `convert(text)`, which returns the dict. Rules:

- Blank lines and lines whose first non-space character is `;` or `#` are skipped. There are no inline comments.
- `key = value` splits on the first `=` (the key must not be empty), and spaces around both the key and the value are stripped. Keys before the first section go at the top level.
- `[name]` starts a section (spaces inside the brackets are stripped). A dotted name nests, so `[db.replica]` gives `{"db": {"replica": {...}}}`. A section that appears again is merged, and when a key repeats, the later value wins. Keys keep the order in which they first appear.
- Values: a value wrapped in double quotes is a string, with the quotes removed and nothing else converted. Otherwise, a value containing a comma is a list of its items, each stripped and converted. Each value or item is then converted: `true`/`false` in any case become booleans, an optional sign plus digits becomes an int, an optional sign plus digits.digits becomes a float, and anything else stays a string (an empty value is "").
- Any other line is an error. `convert` raises `ValueError("line N: cannot parse")` (N counts from 1), and the script prints that message to stderr and exits with status 1.

`python3 ini2json.py examples/service.ini` should reproduce the data in examples/service.json.
