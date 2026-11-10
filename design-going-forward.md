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

## Open questions

1. **Gitignored files.** Checkpoints skip them, so a clobbered `.env` can't be undone. Suggested: checkpoint small ignored files too, outside the usual heavy directories (`target`, `node_modules`, `.venv`).
2. **Planning.** With no plan mode, planning is an ordinary conversation that ends in the plan file. Is a read-only switch still wanted for it, or is undo enough?
3. **Who ticks an item.** Suggested: the harness, when the task ended by itself and the check passed; otherwise the item stays open and the report says why.
4. **Network inside the sandbox.** Suggested: on, since builds fetch dependencies, with credentials hidden.
5. **Limits per task.** How many requests and summaries before a task stops. To be set from use, not guessed.
6. **Keybindings and the optional grep, find and ls tools.** Small and unused. Cut with the rest unless wanted.
