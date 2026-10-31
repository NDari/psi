"""Configuration for the mailer service.

Settings start from DEFAULTS and can be overridden by an INI file:

    [server]
    port = 9000
    debug = yes

Every value is converted to the type of its default. Unknown sections and
keys are errors, so typos do not go unnoticed.

    python3 settings.py [FILE]

prints the effective settings, one "section.key = value" line each.

Environment variables MAILER_<SECTION>_<KEY> override both, for example
MAILER_SERVER_PORT=9000.
"""
import configparser
import os
import sys

DEFAULTS = {
    "server": {
        "host": "127.0.0.1",
        "port": 8025,
        "debug": False,
        "max_connections": 50,
    },
    "smtp": {
        "relay": "localhost",
        "timeout": 10.0,
        "use_tls": True,
    },
    "queue": {
        "dir": "/var/spool/mailer",
        "retry_minutes": 15,
    },
}

TRUE_WORDS = {"1", "true", "yes", "on"}
FALSE_WORDS = {"0", "false", "no", "off"}


class ConfigError(Exception):
    pass


def _coerce(text, default, where):
    """Convert text to the type of default. `where` names the source in errors."""
    text = text.strip()
    if isinstance(default, bool):
        word = text.lower()
        if word in TRUE_WORDS:
            return True
        if word in FALSE_WORDS:
            return False
        raise ConfigError(f"{where}: expected a boolean, got {text!r}")
    if isinstance(default, int):
        try:
            return int(text)
        except ValueError:
            raise ConfigError(f"{where}: expected an integer, got {text!r}") from None
    if isinstance(default, float):
        try:
            return float(text)
        except ValueError:
            raise ConfigError(f"{where}: expected a number, got {text!r}") from None
    return text


def _copy_defaults():
    return {section: dict(values) for section, values in DEFAULTS.items()}


def _apply_file(settings, path):
    parser = configparser.ConfigParser(interpolation=None)
    try:
        with open(path, encoding="utf-8") as f:
            parser.read_file(f)
    except OSError as e:
        raise ConfigError(f"{path}: {e.strerror}") from None
    except configparser.Error as e:
        raise ConfigError(f"{path}: {e}") from None
    for section in parser.sections():
        if section not in settings:
            raise ConfigError(f"{path}: unknown section [{section}]")
        for key, text in parser.items(section):
            if key not in settings[section]:
                raise ConfigError(f"{path}: unknown key {section}.{key}")
            where = f"{path}: {section}.{key}"
            settings[section][key] = _coerce(text, DEFAULTS[section][key], where)


ENV_PREFIX = "MAILER_"


def _apply_env(settings, env):
    for section, values in settings.items():
        for key in values:
            name = f"{ENV_PREFIX}{section}_{key}".upper()
            if name in env:
                values[key] = _coerce(env[name], DEFAULTS[section][key], name)


def load(path=None, env=None):
    """Return the settings as {section: {key: value}}: defaults, then the file,
    then the environment (env, default os.environ)."""
    settings = _copy_defaults()
    if path is not None:
        _apply_file(settings, path)
    _apply_env(settings, os.environ if env is None else env)
    return settings


def format_settings(settings):
    lines = []
    for section, values in settings.items():
        for key, value in values.items():
            lines.append(f"{section}.{key} = {value}")
    return "\n".join(lines) + "\n"


def main(argv):
    try:
        settings = load(argv[1] if len(argv) > 1 else None)
    except ConfigError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    sys.stdout.write(format_settings(settings))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
