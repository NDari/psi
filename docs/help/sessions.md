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
- psi inspect <session> shows a session as a timeline; psi replay <session> --at <n>
  resends one request.
