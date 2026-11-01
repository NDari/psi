Plan mode: read-only exploration that ends with a numbered plan.

Turn it on with /plan (or Ctrl+Alt+P, or psi --plan). While it is on, write and edit
are off and bash runs only read-only commands (ls, cat, grep, find, git status, ...);
anything else is refused. Each prompt reminds the model to finish with:

Plan:
1. First step
2. Second step

When an answer has a plan, psi shows it and asks: carry it out, stay in plan mode, or
refine it. Carrying it out turns plan mode off, sends the steps to the model, and tracks
them: the model writes [DONE:n] as it finishes step n, and the status line shows the
progress. /plan again leaves plan mode without carrying anything out.
