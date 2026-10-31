"""Convert an INI settings file to JSON (the reverse of json2ini.py)."""
import json
import re
import sys

INT_RE = re.compile(r"[+-]?\d+")
FLOAT_RE = re.compile(r"[+-]?\d+\.\d+")


def value_of(text, split=True):
    text = text.strip()
    if len(text) >= 2 and text[0] == '"' and text[-1] == '"':
        return text[1:-1]
    if split and "," in text:
        return [value_of(item, split=False) for item in text.split(",")]
    if text.lower() == "true":
        return True
    if text.lower() == "false":
        return False
    if INT_RE.fullmatch(text):
        return int(text)
    if FLOAT_RE.fullmatch(text):
        return float(text)
    return text


def convert(text):
    result = {}
    current = result
    for n, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line[0] in ";#":
            continue
        if line.startswith("[") and line.endswith("]") and line[1:-1].strip():
            current = result
            for part in line[1:-1].strip().split("."):
                current = current.setdefault(part.strip(), {})
            continue
        key, eq, value = line.partition("=")
        if not eq or not key.strip():
            raise ValueError(f"line {n}: cannot parse")
        current[key.strip()] = value_of(value)
    return result


def main(argv):
    if len(argv) != 2:
        print("usage: ini2json.py FILE.ini", file=sys.stderr)
        return 2
    with open(argv[1], encoding="utf-8") as f:
        text = f.read()
    try:
        data = convert(text)
    except ValueError as e:
        print(e, file=sys.stderr)
        return 1
    print(json.dumps(data, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
