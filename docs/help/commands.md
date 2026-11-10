Slash commands and keys in the interactive mode.

/help lists everything. The main commands:
- /undo: restore the files to before the last prompt that changed them; again steps further back.
- /rewind: go back to an earlier prompt, conversation and files.
- /tree: move to another point in the conversation (files stay).
- /diff: the session's file changes, by source. /show <n>: tool call n in full.
- /stats: tokens, timing, guards and asks. /inspect: the session as a timeline.
- /help, /inspect, /diff and /show open in $PAGER (less if unset).
- /copy: the last answer to the clipboard; /copy code: its last code block (OSC 52;
  in tmux, set -g allow-passthrough on).
- /mode auto|strict, /label <text>, /new, /resume, /fork, /quit.

Keys: Enter sends (while a run is going, it steers it), Alt+Enter queues a follow-up,
Shift+Enter (or Ctrl+J, which works in any terminal) is a new line, Esc aborts, Ctrl+C clears (twice quits), Ctrl+D quits,
Ctrl+G edits the prompt in $VISUAL or $EDITOR,
Ctrl+R searches earlier prompts, Ctrl+S puts the draft aside (again brings it back).
A paste over 10 lines or 1,000 characters shows as [Pasted text #1 +N lines] and is
sent whole; invisible characters (Unicode tags, bidi controls) are removed from pastes. @path adds a file reference; !cmd runs a command
yourself and gives its output to the model (!!cmd keeps it out).

Status line: context used (yellow from 70%, red from 90%), tok/s, prompt read speed,
cache reuse, time to first token; the spinner shows llama-server reading a long prompt,
and says when the model is thinking or writing a tool call, with a size that grows.
File paths and links are clickable (OSC 8; tmux 3.4+: terminal-features ',*:hyperlinks').
Shell completion: in fish, COMPLETE=fish psi | source in config.fish (bash, zsh: the same
with COMPLETE=bash or zsh).
