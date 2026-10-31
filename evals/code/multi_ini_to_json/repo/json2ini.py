"""Convert a JSON settings file to INI.

Nested objects become dotted section names ({"db": {"replica": {...}}} gives
[db.replica]); scalar keys at the top level come first, outside any section.
Lists are written comma separated, booleans as true/false.
"""
import json
import sys


def scalar(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, list):
        return ", ".join(scalar(v) for v in value)
    if value is None:
        return ""
    return str(value)


def to_ini(data):
    out = []

    def section(name, obj):
        plain = [(k, v) for k, v in obj.items() if not isinstance(v, dict)]
        nested = [(k, v) for k, v in obj.items() if isinstance(v, dict)]
        if plain:
            if name:
                if out:
                    out.append("")
                out.append(f"[{name}]")
            for k, v in plain:
                out.append(f"{k} = {scalar(v)}")
        for k, v in nested:
            section(f"{name}.{k}" if name else k, v)

    section("", data)
    return "\n".join(out) + "\n"


def main(argv):
    if len(argv) != 2:
        print("usage: json2ini.py FILE.json", file=sys.stderr)
        return 2
    with open(argv[1], encoding="utf-8") as f:
        sys.stdout.write(to_ini(json.load(f)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
