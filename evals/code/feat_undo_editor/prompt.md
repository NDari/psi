The note editor needs undo and redo. Add them to TextBuffer in buffer.py:

- `undo()` reverts the most recent edit and `redo()` re-applies the most recently undone one. undo restores the text and the cursor to what they were just before that edit, redo to what they were just after it. Both return True, or return False and change nothing when there is nothing to undo or redo.
- Each call of `insert`, `delete` or `replace_all` that changes the text is one undo step. Calls that leave the text unchanged (inserting "", deleting 0 characters, replacing with no matches) add no step and do not clear redo. `move` is not an undo step.
- A new edit clears the redo history.
- Properties `can_undo` and `can_redo` (bools).
- `TextBuffer(text="", history_limit=100)`: keep at most history_limit undo steps, dropping the oldest. history_limit below 1 raises ValueError.
