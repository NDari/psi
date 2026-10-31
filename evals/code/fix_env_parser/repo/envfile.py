"""Read .env files (see README.md for the format)."""
import re

_KEY = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
_COMMENT = re.compile(r"\s#")


def parse(text):
    """Parse the text of a .env file into a dict."""
    env = {}
    for lineno, raw in enumerate(text.splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[len("export "):].lstrip()
        key, value = _split(line, lineno)
        env[key] = _value(value, lineno)
    return env


def load(path, encoding="utf-8"):
    """Parse the .env file at path."""
    with open(path, encoding=encoding) as f:
        return parse(f.read())


def _split(line, lineno):
    if "=" not in line:
        raise ValueError(f"line {lineno}: expected KEY=VALUE")
    parts = line.split("=")
    key, value = parts[0].strip(), parts[1]
    if not _KEY.fullmatch(key):
        raise ValueError(f"line {lineno}: bad key {key!r}")
    return key, value.strip()


def _value(value, lineno):
    value = _strip_comment(value)
    return _unquote(value, lineno)


def _strip_comment(value):
    m = _COMMENT.search(value)
    if m:
        value = value[:m.start()]
    return value.strip()


def _unquote(value, lineno):
    if not value or value[0] not in "\"'":
        return value
    quote = value[0]
    if len(value) < 2 or value[-1] != quote:
        raise ValueError(f"line {lineno}: unterminated quote")
    inner = value[1:-1]
    if quote == '"':
        inner = inner.replace("\\n", "\n").replace('\\"', '"')
    return inner
