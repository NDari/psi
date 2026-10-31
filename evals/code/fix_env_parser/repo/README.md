# envfile

A small reader for `.env` files, so our services don't need python-dotenv.

    from envfile import parse, load
    settings = load(".env")          # {"KEY": "value", ...}

Format, one setting per line:

- `KEY=VALUE`; the key is letters, digits and underscores, not starting
  with a digit. An optional `export ` before the key is ignored.
- Blank lines and lines starting with `#` are ignored.
- Unquoted values are stripped of surrounding spaces, and a ` # comment`
  after them (a `#` preceded by whitespace) is dropped.
- Values may be quoted with `'...'` or `"..."`; the quotes are removed and
  everything between them is kept as is, `#` included. Inside double quotes
  `\n` becomes a newline and `\"` a quote. A comment may follow the
  closing quote.
- A later line overrides an earlier one with the same key.
- Malformed lines raise ValueError naming the line number.
