We want a git pre-commit hook for this repo. Please write `hooks/pre-commit` as a bash script (we install it by copying it to `.git/hooks/pre-commit`). It looks at what is staged for the commit, never the working tree, and rejects the commit when:

1. a staged file is named `.env` in any directory (`.env.example` and the like are fine): message `pre-commit: .env file staged: PATH`
2. a line added by the staged changes to a `.py` file contains `breakpoint()`: message `pre-commit: breakpoint() in PATH`
3. a staged file is larger than 512000 bytes: message `pre-commit: PATH is larger than 500 KB`

PATH is the path from the repository root. Deleted files are never a problem. Print each message once per offending file on stderr, report every problem before exiting, and exit 1 if there were any problems, 0 otherwise. Paths may contain spaces.
