For release notes, please write `changelog.sh`, run inside a git repository as `bash changelog.sh FROM [TO]` (TO defaults to HEAD). It lists the non-merge commits in FROM..TO (reachable from TO but not from FROM), oldest first, grouped into the sections `## Features`, `## Fixes` and `## Other` (in that order), one line `- <description> (<hash>)` per commit. For example:

```
## Features
- ui: dark mode (1a2b3c4)

## Other
- docs: update README (5d6e7f8)
```

A subject `feat: X` goes under Features as `X`, and `feat(SCOPE): X` as `SCOPE: X`; `fix: X` and `fix(SCOPE): X` likewise under Fixes. Every other subject goes under Other unchanged. `<hash>` is the first 7 characters of the commit hash. Leave out sections with no commits; sections are separated by one blank line, with none at the end. If FROM or TO is not a valid revision, print a message on stderr and exit 1.
