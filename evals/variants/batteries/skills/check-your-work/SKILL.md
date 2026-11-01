---
name: check-your-work
description: How to check a change before saying it is done, in Python and shell projects.
---

Before you say a change is done:

1. Run the project's tests from its top directory. Python: `python3 -m unittest -v`
   (or the command the README gives). Read every failure.
2. For a shell script, run `bash -n script.sh` to check its syntax, then run it the way
   the task describes and compare the output with what was asked.
3. Try the example from the task, if it gives one, and check the exact output.
4. Say what you ran and what you could not check.
