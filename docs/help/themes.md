Themes: the interactive mode's colors.

The default uses the terminal's 16 colors, so your terminal's theme decides the shades.
A theme is ~/.psi/themes/<file>.json (or the project's .psi/themes, or a package's) in
pi's format: { "name": "mine", "vars": { ... }, "colors": { "accent": "#ff8800", ... } }.
A color is "" (the terminal's own), 0-255, #rgb or #rrggbb, an ANSI name such as
"brightBlue", or a var. Tokens psi draws: accent, success, error, warning, muted,
userMessageText, bashMode, borderMuted, mdHeading, mdCode, mdCodeBlockBorder,
toolDiffAdded, toolDiffRemoved; others are ignored.

Set "theme": "mine" in settings; /theme lists the themes or switches for the session.
