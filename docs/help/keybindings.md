Keybindings: which keys do what in the interactive mode.

~/.psi/keybindings.json maps pi's action ids to a key or a list of keys; a list
replaces the defaults and [] unbinds. Keys look like ctrl+a, alt+enter, shift+tab,
ctrl+alt+p, f5, pageUp. Example:

{ "app.plan.toggle": "ctrl+g", "app.interrupt": ["escape", "ctrl+q"] }

Actions: tui.editor.cursorUp/Down/Left/Right, cursorWordLeft/Right, cursorLineStart/End,
deleteCharBackward/Forward, deleteWordBackward, deleteToLineStart/End;
tui.input.newLine, tui.input.submit; app.interrupt, app.clear, app.exit,
app.message.followUp, app.plan.toggle. /help shows the keys in use.
