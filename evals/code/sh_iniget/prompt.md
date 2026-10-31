Our deploy scripts need to read single values out of INI files. Please write `iniget.sh`, run as `bash iniget.sh FILE SECTION KEY`, that prints the value of KEY in section [SECTION] followed by a newline.

- Lines whose first non-blank character is `;` or `#` are comments.
- Whitespace around the key, around the `=` and at the ends of the value is ignored. The value may itself contain `=`.
- Section and key names are case-sensitive. A key with the same name in another section does not count.
- If the key appears more than once in the section, the last one wins.
- If the key is not found, print nothing and exit 1. If FILE does not exist, exit 2.

There's an example in config/app.ini.
