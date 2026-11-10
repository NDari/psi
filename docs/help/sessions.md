Sessions: where conversations are kept and how to move around them.

Each session is a JSONL file in ~/.psi/sessions/<directory>/, with a .wire.jsonl next
to it holding the exact requests and responses. Entries form a tree, so going back and
prompting again starts a new branch.

- psi -c continues the latest session here; --session <file> continues that one.
- /resume picks an earlier session; /new starts one.
- /tree moves to any earlier prompt (the files stay as they are); /rewind does that and
  restores the files too. /label names the current point.
- /fork starts a new session from the history before one of your prompts and puts that
  prompt in the editor to edit and send; the original session is left as it was.
- psi inspect <session> shows a session as a timeline.
- psi guards <session or directory>... runs today's guards over recordings and says
  where each would nudge or stop a run (--stops: only runs they would stop).
- psi replay <session> --at <n> sends request n again. --samples <k> draws k
  continuations and tallies what the model does next. Edits try a change against what
  the model really saw: --append <text> adds a user message, --replace <old> <new>
  changes text in the messages or tool descriptions, --drop-last <n> removes the last
  messages, --set temperature=1.0 sets a request field. --count <text> counts the
  samples whose next action contains the text.
