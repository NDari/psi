# Psi: design going forward

Written 2026-11-09 with the owner, to refocus the project. `psi-plan.md` grew into the spec, the changelog and the lab notebook at once, and its goals drifted toward parity with pi. This file says what Psi is for now, what stays, what goes, and in what order. Where it disagrees with `psi-plan.md`, this file wins; the plan stays as history until it is trimmed.

The tag `realign` (commit `7dff330`) marks the code as it was before this change of direction, to go back to if needed.

Status: the direction is agreed (below); the open questions are at the end.

## Agreed on 2026-11-09

- **Refuse, never ask,** to start with. If it proves too tight, adjust; `!cmd` already lets the owner run a command and show the model its output.
- **Extensions and MCP are cut.** The design for them was fine, so they can come back if settings turn out not to be enough.
- **A fresh context per checklist item,** with the checklist held by the harness and given to the model. A summary stays as the safety net for a task that fills the window when it is nearly done.
- **Both Linux and macOS.**
- **One built-in look.** No theme files; the default is checked on gruvbox dark.

## What Psi is for

One person's coding agent for a local model.

- **The work:** features and bug fixes on hobby projects and work that is not security-critical.
- **The model:** whatever runs well on the owner's hardware. Today that is a 27B to 35B model on the Mac. The model will change; the hardware limit won't soon.
- **The way of working:** the owner and the model plan together and write the plan down. The model then carries it out mostly unattended, in tasks of minutes. The owner skims the result, reviews the critical parts, and does the pushes.
- **Where it runs:** Linux and macOS.

## The four goals

Each goal is stated in the form that can actually be reached with a model of this size.

### 1. Safety: nothing the owner can't recover from

- Inside a run, an action is either undoable or impossible. There is no third kind that asks.
- The model may use git: read history, diff, commit, branch. It can't push, and a rewrite of local history can be rolled back.
- The boundary limits what the model can reach, since no parser will outsmart a model that tries. Credentials are out of reach inside it.
- **Done when:** a push from inside a run fails however it is written; `~/.ssh` and git credentials can't be read; a write outside the workspace fails; `/undo` restores the files and the branch tips after any run; a normal run shows no approval prompt.

### 2. Performance: never below the model's weight

- "Punching above its weight" by prompting is not reachable: the best loop note worked in 1 of 3 live runs (`experiments.md`, 2026-11-09). What is reachable is not wasting what the model has.
- That means few tools on every request, no avoidable re-reads of the context, tasks small enough to fit the window, and time shown wherever it is spent.
- **Done when:** a task that fits the window never summarizes; nothing in a normal run makes the server read the context again; each request offers the core tools and nothing else.

### 3. A clear view and an easy way back

- After a run, one screen says what changed, which check ran and how the run ended, written by the harness and not taken from the model's account.
- Changes are easy to see per task, to undo per task, and to rewind to a point and go on from there.
- **Done when:** the owner can walk away, come back, and know in a minute what happened and whether to keep it.

### 4. Getting better with a strong model's help

- Every run leaves enough behind (the session, the wire log) for a strong model to say why it went wrong and to try a change against the recording.
- The default answer to a problem is to work around it: stop, clear, split the task. The harness changes only when a problem comes back on ordinary tasks.
- **Done when:** a question like "why did this loop" or "would this wording help" can be answered from recordings with a command, without a throwaway script.

## The shape

Psi runs one small task at a time, in a fresh context, inside a boundary where nothing unrecoverable can happen, and ends each task with a report the harness wrote. A plan file carries the work from task to task.

- **The plan file** is a markdown file with a checklist, written during planning. The harness reads it, gives the model the plan and the item to do, and ticks the item when the task ends well.
- **Each item starts with a fresh context:** the plan, the item, and the harness's own record of the items before it (what changed, which check passed). Nothing is carried in the model's words.
- **Work is never lost when a context ends.** The files are on disk and checkpointed. What a fresh context drops is the conversation, and the plan file and the record stand in for it.
- **A summary is the safety net, not the engine.** If a task fills the window before it is done, one summary lets it finish. A task that needs more than a couple of them stops and says the item is too big.
- **A run is bounded.** Requests, summaries and repeats each have a limit. At a limit the run stops with a report; it never waits for an answer nobody is there to give.
- **The owner can still feed the model.** `!cmd` runs a command as the owner, outside the boundary, and puts its output in the context. That covers the cases where the model needs something it is refused.

## Rules that follow

1. **Refuse, never ask.** Asking only works when someone is there, and prompts that come often stop being read.
2. **Limit what can be reached, don't classify what is said.** A missing credential stops every push; a parser stops the ones it recognizes.
3. **The harness reports; the model is not asked to.** Checks are run by the harness. The model's claims are not evidence.
4. **No feature that works only if the model chooses it.** The plan's principle 4, without the exceptions made for parity.
5. **One class of model.** Tune for what runs on the hardware now. Keep what makes a new model easy to try.
6. **Nothing is built for strangers.** A public release is not ruled out and not designed for.
7. **Work around first.** A harness change needs the problem seen more than once, on ordinary tasks.

## What is given up, and why

| Given up | Why |
|---|---|
| Parity with pi | A moving target built by a team; neither the owner nor the model used most of it. |
| Long autonomous sessions | The model loops and misreports on long open tasks; nothing tried fixed that. |
| Faithful summaries over many compactions | Each costs 100 to 240 s and carries wrong claims forward; short tasks avoid needing them. |
| A local model writing Psi extensions | The plan's own non-goal, and the source of most of the loop data. |
| Classifying bash by parsing it | About 2,100 lines with known holes; the boundary does the work. |
| One harness for 4B to 35B models | The small models were a hardware workaround. |
| Approval prompts | Unattended runs can't answer them. |

## Keep, change, cut

### Keep as it is

- The agent loop, and the tools read, write, edit and bash.
- Checkpoints with `/undo`, `/rewind`, `/diff`.
- The sandbox on both systems: bubblewrap on Linux, Seatbelt on macOS.
- The session file and the wire log, `psi inspect`, `psi replay`.
- The TUI, print mode and JSON mode.
- The post-edit check, and the guards that stop a run.
- The eval runner, as the way to try a new model.
- `!cmd`.

### Change

| What | From | To |
|---|---|---|
| The ask list | Asks the user | Refuses, with what to do instead. The list stays short and is not grown. |
| Credentials | Readable inside the sandbox | Hidden on both systems: SSH keys and agent, git credentials, the `gh` token, cloud keys. |
| Git history | Rewrites ask | Checkpoints record the branch tips, so a rewrite can be undone. Pushes fail for lack of credentials. |
| Compaction | Clearing, a chain of summaries, merges | One plain summary path, a small limit per task, then stop. The cache-reusing request (D-38) stays. |
| Plan mode and todo | A mode, and a tool the model may use | The plan file and its checklist, run by the harness item by item. |
| Project settings | Need a trust prompt | A project file can't loosen safety (isolation, network, writable directories are the user's), so no trust step. |
| Models | Reference models, sampling in the justfile | One profile per model with its sampling, sent per request; a short trial to compare a new model. |
| The plan document | Spec, changelog and notebook | This file for what is true now; `experiments.md` for findings; the old plan as history. |
| The look | Theme files in pi's format, `/theme` | One built-in look. It already takes the terminal's 16 colors, so a gruvbox dark terminal gives gruvbox colors; the owner checks it by eye there, and anything that reads badly is fixed in the default. |

### Cut

| What | Size | Evidence |
|---|---|---|
| Extensions: the Lua host, the API codegen, `psi ext check` and `test`, packages, trust | about 9.8K lines | The owner's `~/.psi` has none. |
| MCP | 1.6K lines | No call in 12 eval trials; none configured. |
| Sub-agents | part of `psi-agent` | Chosen by the model in 1 of 13 trials; a fresh context per item does the same job. |
| Skills, prompt templates, theme files | part of `psi-cli` | None in the owner's `~/.psi`; skills were never read by the model. |
| The approval card and its paths | part of `psi-agent` and the TUI | Nothing asks. |
| The read-only bash checker | 0.9K lines | Existed for plan mode and read-only sub-agents. |
| Clearing old results | part of the context code | Off by default after it brought on a loop. |
| The pi adapter and comparisons with pi | small | Parity is no longer a goal. |
| The small reference models | no code, a habit | Not what runs daily. Their shape fixes stay; they cost nothing. |
| The public-release seams (§13) | plan text | Not designed for. |

## Order of work

0. **Point the docs at this file.** The working agreements an agent reads at the start of every session still say the plan is the spec, pi is the reference, and to work phase by phase (`AGENTS.md`). Fix those lines first, and put a note at the top of `psi-plan.md` that this file wins. Small, and it stops the old direction from being followed while the rest is done.
1. **Cut.** Everything in the cut list, one area at a time, with the checks passing after each. It comes first because it shrinks every later change. The help topics and docs of a cut feature go with its code.
2. **Safety.** Refuse instead of ask; hide credentials in both sandboxes; record branch tips in checkpoints; decide gitignored files (below).
3. **Tasks.** The plan file and its checklist; a fresh context per item; the report at the end of each task; the simpler compaction.
4. **Trying a model.** Sampling in the profile; a small set of long, ordinary prompts with a simple outcome check; replay tools that run the real guards over recordings and send a recorded request again with an edit.
5. **The view.** Changes and undo per task in the TUI, built on what the report records.
6. **Clean the docs.** Go through every document in the repository so that none states a goal, rule or plan that goes against this file:
   - `psi-plan.md`: keep what is still true and needed as a short reference, move the rest out as history, and drop the phases, the hypotheses that no longer apply and the sections for cut features;
   - `AGENTS.md`: the full rewrite, after the code has changed;
   - `known-issues.md`, `experiments.md`, `fable-suggestions.md`: close or remove entries about cut features, and mark the ones this file answers;
   - `psi-cog-test-issues.md`, `qol-survey.md`, `code-review.md`: keep as dated history or remove, entry by entry where they still matter;
   - `docs/` and the built-in help: only what the binary still does.

   It comes last because the cuts and the new pieces change what is true; step 0 covers the time in between.

Each step ends with `just check` passing and a short note here of what was done.

### Done

- **Step 0 (2026-11-09).** `AGENTS.md` now names this file as the direction, drops pi as the reference and the phase-by-phase rule, and points at "Order of work". `psi-plan.md` opens with a note that this file wins.
- **Step 1 (2026-11-09).** Cut, in this order, with `just check` passing after each: the pi adapter and `--agent pi`; MCP (`psi-mcp`); extensions (`psi-ext`, `psi-ext-api`, the examples, the API docs, the `ext_*` eval tasks, `/reload`); sub-agents; skills, prompt templates, theme files and packages; clearing old results; plan mode, the todo tool and the read-only bash checker; `keybindings.json` and the grep, find and ls tools. The Rust code went from about 53K to about 33K lines, and the workspace from 11 crates to 8. With each feature went its tests, help topics, settings and justfile recipes.
  - Decided on the way: plan mode and todo went now, so there is no planning mode until step 3; open question 6 is answered (cut).
  - Moved to step 2: the approval card and its paths (the ask list still asks), and the rest of project trust, which now covers only `.psi/settings.jsonc`.
  - Changed behavior: when a summary fails or can't make room, old results are no longer cleared as a fallback; the request goes out and the overflow path drops old turns. Sessions recorded with cleared results, plans or todos still open.
  - Model-facing text changed in one place: the step-budget refusal says "summarized" where it said "cleared". No eval was run.
  - Left for later: `psi-ai`'s schema support for nested and untyped parameters (it served MCP and extension tools); `Tool`'s side-by-side reads now cover `read` only.
- **Step 2 (2026-11-09).** Nothing asks before a call any more; `just check` passes.
  - Refuse instead of ask: the ask list is the refuse list (`rules/refuse.jsonc`, settings `refuse` and `refuse_remove`), and a listed command is not run, with what to do instead. The list lost the git rewrites (`git reset --hard`, `clean`, `checkout`, `restore`, `stash`, `branch -D`), which checkpoints now undo; it keeps `git push`, `git stash drop` and `clear`, publishing, `kubectl`, `sudo`, nested psi runs and removing `.git`. A command the parser can't read runs, and bash names the syntax error.
  - File tools: `write` and `edit` are refused outside the workspace and the session's TMPDIR, and run everywhere inside it (gitignored files and the project's `.psi` included). `read` refuses credential files. When bash can't be isolated, bash calls are refused (it used to ask each time).
  - Credentials are hidden in both sandboxes: `~/.ssh`, `~/.gnupg`, git credentials, the `gh` token, cloud and registry keys and the runtime directory with the agents' sockets (`psi-safety/src/secrets.rs`). bash also loses token and key variables and gets an empty git credential helper. Checked on Linux with the real directories: all empty inside, and a fetch over SSH and over HTTPS fails. On the Mac the Seatbelt tests pass (hidden paths unreadable, writes outside blocked, network off); that run found a denied credential read reported as a blocked write, now fixed. The keychain and the SSH agent socket on macOS are still open (see `known-issues.md`).
  - Checkpoints record the project's branch and tag tips and HEAD with each snapshot; `/undo` and `/rewind` put them back and reset the index when HEAD's commit changes. A commit that changes no file is a checkpoint too.
  - Gitignored files (open question 1): checkpoints capture them when small (1 MB each, 2,000 at most), except inside a directory ignored as a whole. This is the suggestion, with those limits.
  - Network (open question 4): on, as it was; a project's file can't change it.
  - Cut with it: the approval card, the blocked-write card (rerun outside, add a directory), session grants, `--allow`, `--mode`, `/mode` and strict mode, project trust (`trust.rs`, `--trust-project`, the startup question). A project's `.psi/settings.jsonc` loads as it is, minus `isolation`, `network`, `cache_dirs` and `refuse_remove`, which only the user's file may set; psi says what it dropped.
  - Kept: the pause card (a guard or a budget asks whether to go on; headless runs stop). It goes with step 3's bounded runs.
  - Model-facing text changed: the system prompt's line on approvals now says such actions are refused, and the refusal texts are new. No eval was run.
  - Decided on the way, for the owner to confirm: a failed checkpoint is recorded and the run goes on; with no git, checkpoints are off with a warning and nothing else changes (it used to switch to strict mode).
- **Step 3 (2026-11-09).** A plan file is carried out one item at a time; `just check` passes.
  - The plan file: any markdown file with a checklist (`- [ ] ...`). `/plan [file]` sends the model a fixed request to write down the plan it worked out with the owner in that form. `/run [file]` in the TUI and `psi run [file]` unattended carry out the open items in order (`psi help plans`).
  - A fresh context per item: a `task_start` entry in the session is a boundary, and no request after it sends what came before. The task's first message is the plan, the harness's record of the items done before in this session (files changed, check result) and the item. It is one session file, so `/undo`, `/rewind` and `/diff` work across tasks.
  - The report, written by the harness (`psi-agent/src/plan.rs`): how the task ended, the files changed and whether git refs moved (from the checkpoints at its start and end), the check, requests, tool calls, refused calls, time, and whether the item was ticked or why not. It is a `task_report` entry, an event, and what the screen and `psi run` print. It keeps the two checkpoint ids, for step 5.
  - The check: when a task ends by itself, the harness runs the project's check (`post_edit_check`) once more. The harness ticks the item when the task ended by itself and the check passed (or none is set); otherwise it takes back a tick the model made, says why, and the plan stops there (open question 3).
  - A run is bounded: at a limit or a guard the run stops with the reason, in the TUI too. The pause card and the approver are gone. A loop still gets the note naming its calls once, then stops. The `limits` setting (requests, tool calls, summaries) holds for a task and a print run; the TUI's chat has no request limit (open question 5: starting values, 40, 200 and 2).
  - The simpler compaction: one summary at a time, written with the earlier one in view and taking its place. No chain, no merges, no `compaction` setting (tail, free form). A run gets two summaries; when the context fills a third time it stops and says the work is too big. The cache-reusing request (D-38) stays, with the from-text fallback for when it can't be used.
  - Planning (open question 2): an ordinary conversation, with undo and no read-only switch.
  - Model-facing text changed: the task prompt and the `/plan` request are new; the summary instruction after an earlier summary asks for one that replaces it, and the "Corrections" heading is gone. No eval was run.
  - Left for later, in `known-issues.md`: no second try after a failed end-of-task check; the record of earlier items is per session; `psi inspect` doesn't show tasks.
- **Step 4 (2026-11-09).** A model's settings live in its profile, and recordings answer questions with a command; `just check` passes.
  - Sampling in the profile: `sampling` in `~/.psi/models.jsonc` holds the model card's values by the server's field names and goes out with every request (the summary requests too). The justfile's flags stay as the server's defaults. `psi eval run --model <id>` gives the trials that profile's sampling and thinking setting and records them in `run.json`.
  - The trial set: `psi eval run --tasks trial`, eight of the existing tasks with the most to read and change, 120 requests each (a set file can carry its own `@steps`, and any upper-case file in `evals/code` is a set), and one real prompt on a real project, `cog_bare_words`. A task can start from a ref in a local git repository (`source`), with a `setup.sh` run on the host and no reference solution; the trial image has Go. `evals/README.md` has the steps for trying a model and for adding such a task. More real prompts should follow.
  - `psi guards <paths>`: today's guards run over recorded sessions, eval trials or whole directories, with the loop's own `Guards` and call signature, and say where each would nudge, add the loop note, or stop, and how many requests earlier than the recording ended. Over the 452 sessions on this machine it stops 21 runs, 17 of them for no progress.
  - `psi replay` with edits: `--append`, `--replace OLD NEW`, `--drop-last N` and `--set key=value` change a recorded request before it is sent again, and `--count TEXT` tallies the samples by a rule given up front. An edit that matches nothing is an error.
  - Sessions record whether a person was at the keyboard (`attended` in `session_info`).
  - No model was run: the tools are tested against the replay server and the recordings on disk.

## Open questions

1. **Gitignored files.** Answered in step 2: small ignored files are checkpointed, except inside a directory ignored as a whole.
2. **Planning.** Answered in step 3: an ordinary conversation that ends in the plan file (`/plan`); undo, no read-only switch.
3. **Who ticks an item.** Answered in step 3: the harness, when the task ended by itself and the check passed; otherwise the item stays open and the report says why.
4. **Network inside the sandbox.** Answered in step 2: on, with credentials hidden.
5. **Limits per task.** Starting values since step 3 (the `limits` setting: 40 requests, 200 tool calls, 2 summaries). Still to be set from use.
6. **Keybindings and the optional grep, find and ls tools.** Small and unused. Cut with the rest unless wanted.
