"""A tiny template engine with a registry of filters."""

import re

from .layout import align, truncate
from .normalize import normalize_space, slugify, title_case
from .stats import word_count

#
# Templates use {{ expression }} with an optional chain of filters:
#
#     Hello {{ user.name | title }}, you have {{ count | default:0 }} notes.
#
# A filter may take arguments after a colon, separated by commas. Text
# between {{! and }} is a comment. A backslash before {{ prints {{ as is.

FILTERS = {}


class TemplateError(Exception):
    """Raised for malformed templates, unknown filters and, in strict mode,
    missing variables."""


def register_filter(name):
    """Decorator that adds a function to FILTERS under name.

    The function gets the current value first, then the filter arguments as
    strings.

        @register_filter("shout")
        def shout(value):
            return str(value).upper() + "!"
    """

    def decorator(func):
        FILTERS[name] = func
        return func

    return decorator


_TAG_RE = re.compile(r"(\\?)\{\{(.*?)\}\}", re.S)

_MISSING = object()


def _lookup(context, dotted):
    """Resolve a dotted name such as "user.name" against context."""
    value = context
    for part in dotted.split("."):
        if isinstance(value, dict):
            value = value.get(part, _MISSING)
        elif isinstance(value, (list, tuple)) and part.isdigit():
            idx = int(part)
            value = value[idx] if idx < len(value) else _MISSING
        else:
            value = getattr(value, part, _MISSING)
        if value is _MISSING:
            return _MISSING
    return value


def _parse_filter(spec):
    """Split "name:arg1,arg2" into ("name", ["arg1", "arg2"])."""
    name, _, rest = spec.partition(":")
    name = name.strip()
    args = [a.strip() for a in rest.split(",")] if rest.strip() else []
    return name, args


class Template:
    """A parsed template that can be rendered many times."""

    def __init__(self, source, strict=False):
        self.source = source
        self.strict = strict
        self._parts = self._parse(source)

    @staticmethod
    def _parse(source):
        """Split source into literal strings and (expression, filters) tags."""
        parts = []
        pos = 0
        for m in _TAG_RE.finditer(source):
            parts.append(source[pos : m.start()])
            pos = m.end()
            if m.group(1):
                parts.append("{{" + m.group(2) + "}}")
                continue
            body = m.group(2).strip()
            if body.startswith("!"):
                continue
            if not body:
                raise TemplateError("empty tag at offset %d" % m.start())
            pieces = [p.strip() for p in body.split("|")]
            expr = pieces[0]
            if not re.fullmatch(r"[A-Za-z_][\w.]*", expr):
                raise TemplateError("bad expression %r" % expr)
            filters = [_parse_filter(p) for p in pieces[1:]]
            for fname, _ in filters:
                if not fname:
                    raise TemplateError("empty filter in %r" % body)
            parts.append((expr, filters))
        tail = source[pos:]
        if "{{" in tail.replace("\\{{", ""):
            raise TemplateError("unclosed tag")
        parts.append(tail)
        return parts

    def render(self, context=None, **extra):
        """Render with context (a dict) plus keyword arguments."""
        ctx = dict(context or {})
        ctx.update(extra)
        out = []
        for part in self._parts:
            if isinstance(part, str):
                out.append(part)
                continue
            expr, filters = part
            value = _lookup(ctx, expr)
            if value is _MISSING:
                if self.strict and not any(f == "default" for f, _ in filters):
                    raise TemplateError("missing variable %r" % expr)
                value = None
            for fname, args in filters:
                func = FILTERS.get(fname)
                if func is None:
                    raise TemplateError("unknown filter %r" % fname)
                value = func(value, *args)
            out.append("" if value is None else str(value))
        return "".join(out)


def render(source, context=None, strict=False, **extra):
    """Parse and render a template in one call.

    >>> render("Hi {{ name | upper }}!", {"name": "ada"})
    'Hi ADA!'
    """
    return Template(source, strict=strict).render(context, **extra)


def _text(value):
    return "" if value is None else str(value)


@register_filter("upper")
def _f_upper(value):
    return _text(value).upper()


@register_filter("lower")
def _f_lower(value):
    return _text(value).lower()


@register_filter("title")
def _f_title(value):
    return title_case(_text(value))


@register_filter("slug")
def _f_slug(value, sep="-"):
    return slugify(_text(value), sep=sep)


@register_filter("trim")
def _f_trim(value):
    return normalize_space(_text(value))


@register_filter("truncate")
def _f_truncate(value, width="30"):
    return truncate(_text(value), int(width))


@register_filter("pad")
def _f_pad(value, width, how="left"):
    return align(_text(value), int(width), how)


@register_filter("default")
def _f_default(value, fallback=""):
    return fallback if value is None or value == "" else value


@register_filter("count")
def _f_count(value):
    return word_count(_text(value))


@register_filter("join")
def _f_join(value, sep=", "):
    if value is None:
        return ""
    return sep.join(_text(v) for v in value)


@register_filter("plural")
def _f_plural(value, singular="", plural="s"):
    try:
        n = int(value)
    except (TypeError, ValueError):
        raise TemplateError("plural needs a number, got %r" % (value,))
    return singular if n == 1 else plural
