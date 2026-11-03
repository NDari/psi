Slash commands and keys in the interactive mode.

/help lists everything. The main commands:
- /undo: restore the files to before the last prompt that changed them; again steps further back.
- /rewind: go back to an earlier prompt, conversation and files.
- /tree: move to another point in the conversation (files stay).
- /diff: the session's file changes, by source. /show <n>: tool call n in full.
- /stats: tokens, timing, guards and asks. /inspect: the session as a timeline.
- /mode auto|strict, /plan, /todos, /mcp, /theme, /label <text>, /new, /resume, /fork, /reload, /quit.
- /<template> args runs a prompt template; /skill:<name> args puts a skill in the conversation.

Keys: Enter sends (while a run is going, it steers it), Alt+Enter queues a follow-up,
Shift+Enter is a new line, Esc aborts, Ctrl+C clears (twice quits), Ctrl+D quits,
Ctrl+Alt+P toggles plan mode, Ctrl+G edits the prompt in $VISUAL or $EDITOR. @path adds a file reference; !cmd runs a command
yourself and gives its output to the model (!!cmd keeps it out). Rebind keys in
~/.psi/keybindings.json (psi help keybindings).
