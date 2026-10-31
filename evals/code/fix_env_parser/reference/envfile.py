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
    key, _, value = line.partition("=")
    key = key.strip()
    if not _KEY.fullmatch(key):
        raise ValueError(f"line {lineno}: bad key {key!r}")
    return key, value.strip()


def _value(value, lineno):
    if value and value[0] in "\"'":
        return _unquote(value, lineno)
    return _strip_comment(value)


def _strip_comment(value):
    m = _COMMENT.search(value)
    if m:
        value = value[:m.start()]
    return value.strip()


def _unquote(value, lineno):
    quote = value[0]
    end = value.find(quote, 1)
    if quote == '"':
        while end != -1 and value[end - 1] == "\\":
            end = value.find(quote, end + 1)
    if end == -1:
        raise ValueError(f"line {lineno}: unterminated quote")
    rest = value[end + 1:].strip()
    if rest and not rest.startswith("#"):
        raise ValueError(f"line {lineno}: text after closing quote")
    inner = value[1:end]
    if quote == '"':
        inner = inner.replace("\\n", "\n").replace('\\"', '"')
    return inner
