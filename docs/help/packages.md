Packages: extensions, skills, prompts and themes from a directory or a git repository.

- psi install <source> [-l]: a local directory, or git:github.com/user/repo[@ref],
  https://..., ssh://... -l records it in the project's .psi/settings.jsonc instead of
  ~/.psi/settings.jsonc. Git packages are cloned under ~/.psi/git (or .psi/git).
- psi remove <source> [-l]; psi update [source] (pinned refs stay on their ref; a
  project's packages only once you've trusted the project); psi list.

A package's resources: package.json's "psi" key (extensions, skills, prompts, themes:
lists of paths), or else the extensions/, skills/, prompts/ and themes/ directories.
Skills, prompts and themes from pi packages load too; pi's TypeScript extensions don't.
