In settings.py, let environment variables override the settings too, for running in containers. The order becomes defaults, then the INI file, then the environment.

- `load(path=None, env=None)`: env is a mapping of variable names to strings; None means os.environ.
- The variable for a setting is `MAILER_` + section + `_` + key, upper case: `MAILER_SERVER_PORT`, `MAILER_SMTP_USE_TLS`, `MAILER_SERVER_MAX_CONNECTIONS`.
- Values are converted exactly like file values (same `_coerce`). A bad value raises ConfigError whose message contains the variable name.
- Variables that do not name a known setting are ignored, even when they start with `MAILER_`.
- `python3 settings.py [FILE]` uses os.environ, so its printed settings include the overrides.
