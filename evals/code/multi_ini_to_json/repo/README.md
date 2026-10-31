# confkit

Small tools for moving our service settings between formats.

- `json2ini.py FILE.json` prints the INI form of a JSON settings file.
- `ini2json.py FILE.ini` (planned) does the reverse.

Example settings are in examples/. Tests: `python3 -m unittest discover -s tests`
