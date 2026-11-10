# Psi (ψ): reference

How the parts of Psi work, as built. `design-going-forward.md` says what Psi is for and what comes next; this file is the detail behind it, and where the two disagree that file wins.

- **Section numbers are kept from the old plan**, because comments in the code cite them (`§8.4`, `§7.4`). Sections for features that were cut say so in one line. The decisions the code cites (`D-xx`, `G-xx`, `H-xx`) are in §17.
- **The old plan** (the spec up to 2026-11-09, with its phases, hypotheses and dated results) is `docs/history/psi-plan-2026-11-09.md`. It is history: don't follow it.
- **Findings from runs** go in `experiments.md`, deferred defects in `known-issues.md`.

## 1. What Psi is

One person's coding agent for a local model on llama-server, written in Rust. The executable is `psi`, the crates are `psi-*`, the config directory is `~/.psi/`. Goals, rules and what was given up: `design-going-forward.md`.

Psi defends against a well-meaning model that makes mistakes: wrong edits, bad commands, loops. It does not defend against hostile repositories or prompt injection.

## 2. Decisions at a glance

| Area | Decision |
|---|---|
| Host | Rust (stable, edition 2024), tokio, one binary with the help topics embedded |
| TUI | A custom inline renderer on crossterm, lines committed once (D-03) |
| Config | JSONC (D-04) |
| Undo | A shadow git repository outside the workspace, through the `git` CLI (§7.2) |
| Boundary | bubblewrap on Linux, Seatbelt on macOS; evals run in podman with isolation off (§8.4) |
| What is refused | A short refuse list, writes outside the workspace, credentials (§8) |
| Tool calls | Native tool calls, a startup check of the chat template, shape fixes (D-09) |
| Record | The session file and its wire log are the only record (D-31) |
| Checks | `just check`, run locally; no hosted CI (D-33) |

## 3. Architecture

### 3.1 Workspace layout

```
crates/
  psi-ai/        # The provider client, streaming, tool-call shape fixes, model profiles, wire log
  psi-agent/     # The agent loop, limits, guards, summaries, plans and tasks, recovery
  psi-tools/     # read, write, edit, bash
  psi-safety/    # The refuse list, policy, sandbox, hidden credentials, checkpoints
  psi-session/   # The session file: a tree of JSONL entries
  psi-tui/       # Terminal UI: renderer, editor, markdown
  psi-lab/       # inspect, replay, guards over recordings, the eval runner
  psi-cli/       # The binary: modes, commands, settings, help
docs/help/       # Help topics (embedded in the binary; 30 lines each at most)
docs/history/    # Dated documents that no longer describe the code
rules/           # The shipped refuse list (embedded)
evals/           # Task sets and graders
```

### 3.3 Concurrency

- One tokio runtime for I/O: the model server, tools and files.
- The TUI's render and input loop has its own thread and talks to the agent over channels, so typing never waits on the agent.

### 3.4 Files and directories

| Path | What |
|---|---|
| `~/.psi/settings.jsonc` | User settings |
| `<project>/.psi/settings.jsonc` | Project settings; loaded without asking, minus the keys that could loosen safety (§8.1) |
| `~/.psi/models.jsonc` | Model profiles (§4.3) |
| `~/.psi/sessions/<cwd-slug>/<timestamp>_<id>.jsonl` | Session files (§7.1) |
| `~/.psi/sessions/<cwd-slug>/<timestamp>_<id>.wire.jsonl` | That session's wire log (§4.6) |
| `~/.psi/checkpoints/<hash>/` | Shadow repositories (§7.2) |
| `~/.cache/psi/tmp/<session-id>/` | The session's `TMPDIR` inside bash |
| `~/.psi/evals/<UTC time>-<label>/` | Eval run records (§12.5) |

Settings layers, lowest first: built-in, user, project, command-line flags. Objects merge by key, other values replace, the refuse list appends. `psi help settings` lists the keys.

## 4. The model server (`psi-ai`)

### 4.1 Provider

OpenAI-compatible Chat Completions with streaming, built for and tested against llama-server (llama.cpp).

### 4.2 llama-server specifics

- **Startup check (D-09):** `GET /props` gives whether the chat template supports tool calls (Psi refuses to start a run without it), the context window per slot, and the model's file name for the status line.
- **No server answering:** print runs and evals stop with a clear message. The TUI starts anyway, says "no model server", keeps the prompt in the box and connects when the server answers.
- **Requests are append-only**, so the server's prompt cache hits. One system message, at the start.
- **Template errors are deterministic 500s** and are not retried.
- **Thinking** is the profile's `reasoning.enable_thinking`, sent per request (H-09: off did as well with less output on the 35B).
- **Context overflow:** the server answers HTTP 400 `exceed_context_size_error`; Psi drops the oldest turns until the request fits and records it.

### 4.3 Model profiles (`models.jsonc`)

One profile per model: `id`, `base_url`, `context_window` (`"auto"` reads `/props`), `max_output`, `reasoning`, and `sampling`, the model card's values by the server's own field names, sent with every request. The first profile is the default; `--model <id>` picks another. `psi help models` has an example.

### 4.4 Tool-call shape fixes (D-09)

On llama-server the server parses tool calls, so mistakes arrive as wrong shapes in valid JSON. Every call goes through:

1. **A call cut off by the output limit is never run.**
2. **The tool name is resolved:** exact, then an alias table (`read_file` → `read`, `shell` → `bash`), then edit distance when unambiguous.
3. **Clear mistakes are accepted:** misplaced keys moved when only one reading exists, `"10"` coerced to `10`, `~` expanded in paths.
4. **Refused with the exact fix** when a silent correction would make the file tools disagree with bash (a relative path that repeats the working directory).
5. **Validated;** a failure returns a short error with one correct example call, never a schema dump.
6. **Schemas are flat, and the key that must come first sorts first** (H-05): some chat templates list keys alphabetically, which is why write's text key is `text`.

### 4.5 Context accounting (D-20)

Usage from the response when present, else the server's tokenize endpoint, else 3 characters per token. The context window comes from `/props`.

### 4.6 Wire log

Every request is logged in the exact bytes sent and every streamed response in the bytes received, next to the session file. It is what makes `psi replay` and `psi guards` possible.

## 5. The agent loop (`psi-agent`)

- **A run** starts from a prompt and repeats **turns** until the model answers without a tool call, or a limit or guard stops it. A turn is one model request and the tool calls it returns.
- **A run is bounded and never asks.** Limits (the `limits` setting): 40 requests, 200 tool calls and 2 summaries for one task of a plan or one print run (H-13). At a limit the run stops with the reason. The TUI's own chat has no request or call limit (Esc stops it); the summary limit and the guards hold there too.
- **Queues:** a steering message typed during a run goes in before the next request; a follow-up waits until the run settles.
- **Abort:** Esc cancels the request and the running tools and records what there was.
- **Plans and tasks:** `psi run plan.md` (or `/run`) carries out a markdown checklist one item per task. Each task starts with a fresh context: the plan, the item, and the harness's reports on the items before it. At the end the harness runs the check (`post_edit_check`), writes the task's report, and ticks the item when the task ended by itself and the check passed. Otherwise the item stays open and the plan stops. `psi help plans`.

## 6. Tools (`psi-tools`)

| Tool | Behavior |
|---|---|
| `read(path, offset?, limit?)` | The lines as they are, without numbers. The truncation notice says how to continue. Results are capped relative to the context window. Credential files are refused. |
| `write(path, text)` | Overwriting needs the file read this session and unchanged since; there is no override flag, and the refusal carries the current content and counts as the read (D-24). Only inside the workspace and the session's `TMPDIR`. |
| `edit(path, old_text, new_text)` or `edit(path, edits)` | Exact matches, and matches differing only in trailing whitespace or line endings, apply; anything else returns the closest region and names the difference (D-10). An edit that changes nothing fails. Several edits in one call are matched against the file as it was, and none is made unless all match. |
| `bash(command, timeout?)` | Runs inside the sandbox (§8.4), a fresh process each time. Output is cut head and tail with a notice. A command on the refuse list is not run (§8.1). |

bash, write and edit take an optional one-line `intent`, recorded and never required.

**`!cmd`** in the editor runs a command as the user, outside the sandbox, and gives its output to the model (`!!cmd` keeps it out).

## 7. Sessions, checkpoints and summaries

### 7.1 Sessions (`psi-session`)

- One JSONL file per session. Every entry has `id`, `parent_id`, `type` and `ts`, so a session is a tree.
- Entry types: `message`, `tool_result`, `custom` (not sent to the model), `custom_message` (sent), `compaction`, `label`, `checkpoint`, `policy_decision`, `guard_event`. (`cleared_results` is still read, for sessions recorded before clearing was cut.)
- A plan's tasks are `custom` entries: `task_start` is where a task's context begins, `task_report` is the harness's report.
- Commands: `/new`, `/resume`, `/tree`, `/fork`, `/label`.
- The session file plus its wire log is the full record (D-31). Evals keep both and inspection reads them.

### 7.2 Checkpoints

One bare shadow repository per project under `~/.psi/checkpoints/`, with the project as its work tree. The project's own `.git` is never written by a capture.

- **Capture** at prompt submit and after every step that may have changed files. Skipped: files over 50 MiB, unreadable files, nested repositories. Small gitignored files (up to 1 MiB, 2,000 at most) are captured unless their whole directory is ignored.
- **Git refs:** each checkpoint records the project's branch and tag tips and where HEAD points, so a commit, a branch move or a history rewrite by the model can be undone.
- **Restore** checkpoints the current state first (so it can be undone), writes the files that differ, deletes the extra ones and puts the recorded refs back. A branch made since is left alone.
- **Commands:** `/undo` (before the last prompt that changed something; again steps further back), `/rewind` (conversation and files), `/diff` (the session's changes by source), and per task `/tasks`, `/diff task <n>`, `/undo task <n>`.

### 7.3 Host git hardening

Psi's own git calls run on the host, and bash can write the workspace's `.git/config`. They run with `core.fsmonitor=false`, `core.hooksPath=/dev/null`, `GIT_CONFIG_NOSYSTEM=1` and no inherited `GIT_*` variables; checkpoint repositories also ignore the global config and attributes, so the stored bytes are the bytes on disk.

### 7.4 Summaries

A summary is the safety net, not the engine: tasks are meant to be small enough not to need one.

- **Trigger:** when the room left would fall below one answer plus one tool result, or usage passes 70% of the window, whichever comes first.
- **One summary at a time.** Everything before a verbatim tail of 10K tokens is summarized; a later summary is written with the earlier one in view and takes its place.
- **The request extends the last one (D-38):** the last real request unchanged, with the summary instruction appended, so the server reuses its prompt cache. If that can't work (no room, a tool call, a cut-off answer), a plain request built from text is the fallback.
- **Without thinking:** when the profile has thinking on, the summary request alone goes out with it off, so thinking can't use up the output limit before the summary is written. On a chat template that puts the thinking setting in the system message (Qwen3.8), the server reads the context again for that request.
- **Headings (H-17):** goal, constraints and facts, learned about the code, decisions, current state split into checked and not checked, corrections, open problems, next step.
- **The harness adds its own record** after the summary: files read and changed, and the last post-edit check. Then one line tells the model to go on from the summary's next step.
- **A limit:** a run gets 2 summaries; when the context fills again it stops and says the work is too big for one run.
- **One step's reads have a budget:** read-only results past what one step can keep in the window are not given to the model; the note says to use the earlier ones first and make the call again.
- `/compact [what to keep]` summarizes by hand.

## 8. Safety (`psi-safety`)

Nothing asks. Inside a run an action can be undone, or it is refused. `psi help safety` is the user's version of this section.

| Mechanism | What it covers |
|---|---|
| Undo (checkpoints, §7.2) | Any change inside the workspace, and local git history |
| Boundary (the sandbox, §8.4) | bash can't write outside the workspace, its git directories, the cache directories and its `TMPDIR` |
| Hidden credentials | SSH keys and agent, git credentials, the `gh` token, cloud and registry keys can't be read, so a push fails however it is written |
| The refuse list | A few commands that can't be undone and aren't stopped by the above |

### 8.1 What is refused

- **A bash command containing a refuse-list entry** (`rules/refuse.jsonc`): `git push`, dropping stashes, publishing and deploying, `kubectl`, `sudo`, nested `psi` runs, and removing or moving the repository's `.git`. Entries are found anywhere in a command (pipelines, substitutions, wrappers such as `timeout` or `sh -c`), with tree-sitter-bash. The list is short on purpose and is not grown: the boundary and the hidden credentials do the real work. The model is told to leave the action to the user.
- **A file write outside the workspace and the session's `TMPDIR`.**
- **A read of a credential file.**
- **Every bash call when the sandbox can't start** (the status line says `BASH OFF`).

Settings: `refuse` appends entries, `refuse_remove` drops shipped ones. A project's settings file can't set `isolation`, `network`, `cache_dirs` or `refuse_remove`; Psi says so at startup.

### 8.4 The sandbox

**Linux: bubblewrap.** `/` read-only; writable binds for the workspace, its git directories outside it, the cache directories and the session's `TMPDIR` (which is also `/tmp`); `--unshare-pid`, `--new-session`, `--die-with-parent`; `--unshare-net` when `network` is off. Credential paths are covered with an empty tmpfs or `/dev/null`.

**macOS: a Seatbelt profile** (`sandbox-exec`, D-44) made from the same directories: writes denied outside them, reads and writes denied on the credential paths, `(deny network*)` when the network is off. Paths are resolved first (`/tmp` is a link into `/private`). `/tmp` and `/private/var/folders` stay shared and writable, since macOS tools write there whatever `TMPDIR` says.

- **Cache directories** default to `~/.cache`, `~/.cargo/registry`, `~/.cargo/git`, `~/.npm`, `~/go/pkg/mod` and `~/Library/Caches` (`cache_dirs`). `/` and the home directory are refused as writable directories.
- **Environment:** token and key variables and agent sockets are dropped, with `BASH_ENV` and `ENV`; git gets an empty credential helper. `TMPDIR` is the session's, and `PAGER=cat`, `GIT_TERMINAL_PROMPT=0` and `TERM=dumb` keep anything from waiting for input.
- **Startup check:** the sandbox is run once with the real flags. If it fails, Psi prints the error and bash is refused; it never falls back silently.
- **Known gaps:** `known-issues.md` (the macOS keychain, reads outside the credential paths, files in wholly ignored directories).
- **Tests:** the Seatbelt tests run only on macOS, by hand.
- **Evals** run Psi in a podman container with isolation off; the container is the boundary.

### 8.5 When a write is blocked (D-23)

A bash command that fails on a read-only path outside every writable directory is recorded as blocked, and the model gets a short message with one next step: say what was needed instead of trying variants. The model has no flag to leave the sandbox. The user can run the command with `!cmd`.

### 8.6 Guards

Each firing is a `guard_event` in the session. `psi guards <paths>` runs the same guards over recordings.

- **Repeated failure (H-12):** the same call failing 3 times in a row gets one line in its result; at 5 the run stops.
- **Error streak:** 10 failed calls in a row get a line to re-read and re-plan; 20 stop the run. A `grep` that matched nothing is not an error.
- **No progress (G-12):** 6 turns in a row that only repeat earlier calls and change no file get a line; 12 stop the run. Repeats are exact: the tool and its arguments, without `intent`. At the first stop the model gets a note naming the repeated calls and the run goes on once; the second stop ends it. What this misses is in `known-issues.md`.
- **Announced stop (unattended runs):** an answer that ends by announcing a next step and makes no call gets one line, once.
- **Cut-off answer:** a reply that hits the output limit before any call gets one line and another turn, twice at most.

## 9. Extensions

Cut on 2026-11-09 (the Lua host, the API codegen, packages). History: `docs/history/psi-plan-2026-11-09.md` §9.

## 10. What ships in the binary

- **Plans and tasks** (§5), with `/plan`, `/run`, `/tasks`.
- **`/diff`, `/undo`, `/rewind`** (§7.2).
- **The post-edit check (D-36):** the project's check command (`post_edit_check`), run once after each turn whose write or edit calls changed files, with a short result for the model; also the check at the end of each task.
- **The status line:** model, context used, tokens per second, prompt read speed, cache reuse, time to first token, network, checkpoints, and `BASH OFF` when bash can't be isolated.
- **Help (D-28):** `psi help topics` and `psi help <topic>` print short answers from the embedded help files, and one prompt line tells the model to run them when asked about Psi.

Cut: plan mode, the todo tool, skills, prompt templates, theme files, keybinding files, sub-agents (§10.1) and MCP (§10.2).

## 11. The TUI (`psi-tui`)

### 11.1 Renderer

A line-based differential renderer on crossterm, inline (no alternate screen), with the input box pinned to the bottom rows (D-03).

### 11.2 Commit-once rules

An inline TUI can't redraw the past, so each piece is committed once, in a form that stays useful.

- Text is committed a paragraph at a time, code blocks whole.
- A tool call is committed as a header, a one-line summary, up to 4 output lines and up to 20 diff lines; `/show <n>` displays it in full.
- The mark before a call is dim while it runs, green when it worked, red when it failed. The user's messages have a bar down the left.
- Thinking is never committed. The spinner says what the model is writing when none of it shows (thinking, or a tool call's arguments), with a size that grows.
- Committed lines are not redrawn on resize.
- The output of `/inspect`, `/diff`, `/show` and `/help` opens in `$PAGER`.
- Dropdowns go under the input box; the status line is the last row. What the box shows is what Enter runs.
- Colors are the terminal's 16, so the terminal's theme decides. There is one built-in look.
- Markdown is drawn for reading: headings without their `#`s, a single newline as a line break, a table wider than the terminal as its rows written out. `/copy` gives the raw markdown.

### 11.3 Feel targets

| Metric | Target |
|---|---|
| Cold start to interactive, without the model | under 100 ms |
| Keystroke to screen | under 16 ms, whatever the agent is doing |
| Streaming | No flicker; output at the model's rate |
| Long sessions | 10K committed lines with no input lag |

### 11.4 Editor

A multi-line editor with history, `@` file references with fuzzy completion, `!cmd`, slash-command completion, steering and follow-up input while a run is going. `psi help commands` lists the keys.

### 11.5 Markdown and code highlighting (D-43)

The stream is split into blocks (paragraphs, headings, whole fenced code blocks) and each block is rendered once, when complete. Prose goes through `pulldown-cmark`; fenced code with a language tag is colored by `syntect`, mapped onto the 16 ANSI colors. Only color is added, so a copy from the terminal is still the code.

## 12. Inspection and trying a model (`psi-lab`)

### 12.2 `psi inspect <session|trial>`

A timeline: each turn's text, tool calls, results, policy decisions, guard events, tokens and timing, a plan's task starts and reports. Filters: `--failures`, `--guards`, `--blocked`, `--step N`; `--json`. Patterns are flagged (loops, retries after a refusal, truncated calls, overflows).

### 12.4 `psi replay <session> --at <step>`

Sends request N again from the wire log. `--samples K` draws K continuations and tallies the next action. Edits change the request first: `--append`, `--replace OLD NEW`, `--drop-last N`, `--set key=value`; `--count TEXT` counts the samples whose next action contains the text. This is how a change to model-facing text is tried before it is built.

`psi guards <paths>` runs today's guards over recorded sessions and says where each would nudge or stop.

### 12.5 `psi eval`

Coding tasks, each trial in a fresh podman container with no network but the model server; grading runs with no network. `evals/README.md` is the manual.

- **What it is for:** sanity checks, finding harness bugs, and trying a new model (`psi eval run --tasks trial --model <id>`). It is not a benchmark.
- **Outcomes:** pass, model failure, harness failure. A harness failure never counts as a model result.
- **A run is one trial per task and proves correctness only.** Never use one to judge speed or how well a model does, and don't repeat it for more trials.
- **When to run it (D-32):** when model-facing text changes, when a model changes, or to refresh replay fixtures. Tests cover everything else.
- **Task sets (G-19):** the quick set (`evals/code/SET`) and the trial set (`evals/code/TRIAL`); replace tasks that give the same result every time.

## 16. Tests

- Every behavior change gets a test. Scenario tests drive the real loop with the replay server (`psi_ai::replay`), playing back recorded llama-server responses.
- No test touches the network or a real model.
- `just check` runs fmt, clippy with warnings denied, the tests, and the eval pipeline through podman with the replay server as the model.

## 17. Decisions the code cites

The full log, with the superseded ones, is in the old plan.

| ID | Decision |
|---|---|
| D-03 | A custom inline differential renderer on crossterm, with commit-once rules |
| D-04 | JSONC config |
| D-07, D-08 | bubblewrap as a good-enough boundary on Linux; network on by default with an `off` switch |
| D-09 | Native tool calls, with a `/props` startup check and the shape fixes |
| D-10 | Edits apply on exact and trailing-whitespace or line-ending matches only; anything else gets a hint |
| D-18 | Unattended runs fail closed. Since 2026-11-09 nothing asks in any mode |
| D-20 | Tokens: usage, then the tokenize endpoint, then 3 characters per token |
| D-23 | The model has no flag to leave the sandbox |
| D-24 | No force flag on write; a read since the last change is required |
| D-28 | A prompt line that says to run `psi help` when asked about Psi |
| D-29 | Reference models. Superseded: one class of model, a profile each (`design-going-forward.md`) |
| D-30 | Good enough first, stricter later: simple mechanisms with known gaps |
| D-31 | The session file plus its wire log is the only record |
| D-32 | Tests carry everyday checking; the eval runs sparingly |
| D-33 | Checks run locally; no hosted CI |
| D-36 | The post-edit check runs once per turn that edited files |
| D-38 | The summary request extends the last request, to reuse the server's prompt cache |
| D-40 | A chain of summaries. Superseded 2026-11-09: one summary at a time (§7.4) |
| D-43 | Markdown through `pulldown-cmark`, code through `syntect` |
| D-44 | macOS isolation is a Seatbelt profile with a shared `/tmp` |
| G-12 | The no-progress guard's counts (§8.6) |
| G-19 | Replace eval tasks that give the same result every trial |
| H-04 | Models retry refused calls in other forms: counted (`retry_after_block`), seen only on a 4B model |
| H-05 | Alphabetical key order in a chat template misleads some models: flat schemas and key naming (§4.4) |
| H-09 | Thinking on gave no gain at higher cost on the 35B; the profile decides |
| H-12 | A one-line nudge after 3 identical failures; the stop at 5 is what bounds it |
| H-13 | 40 requests is enough for a small task |
| H-17 | Summary headings for checked and not checked, what was learned, and corrections |

## Appendix B: the system prompt

The text is in `crates/psi-agent/src/prompt.rs`. Every line costs context on every request: keep it short, and try a change with `psi replay` and then the eval before keeping it.
