The todo tool: a task list the model keeps for the session.

Off by default; set "todo": true in settings to offer it. The model calls todo with
action list, add (text), toggle (id) or clear. The list lives in the session and follows
the conversation branch, so /tree and /rewind bring back the list as it was there.
/todos shows the current list.
