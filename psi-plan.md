# Heimdall v2 — Implementation Plan

> A pi-style coding agent harness in **Rust + mlua**, designed for **local, under-powered LLMs** that will make mistakes.
> v2 is a rewrite of Heimdall v1, which was written in Go. The executable is `heimdall`.
> Goal: give the model the freedom to do almost anything, **with intent** — not pi's "wild west," and not a cage.

**How to use this document (for you and Claude Code)**

- Work phase by phase (§13). Each phase has acceptance criteria; don't start the next phase until they pass.
- Items marked **`[D-xx]`** are *decisions* with a recommendation. Confirm or change them before (or while) implementing the affected phase. Record the outcome inline: `**Decided:** …`.
- Items marked **`[G-xx]`** are *gaps*: things not yet designed that need research or input.
- §15 indexes every `[D-xx]` and `[G-xx]`. Keep it in sync.
- "Pi parity" notes show where we deliberately mimic pi. Pi's source (`earendil-works/pi`, MIT) is the reference for behavior. Read it, but don't copy prompts or docs verbatim; write our own.
- Naming: the project is **Heimdall v2**. The binary is `heimdall`, crates are `heimdall-*`, the config directory is `~/.heimdall/`, and the Lua global is `heimdall` (**[D-01]**, decided).

---

## 0. Lessons from Heimdall v1 (Go) — **[G-15]** to fill in

To be gathered from the v1 project. Tag each lesson with the section of this plan it affects, and update that section.

- **What fell short of the goal?** → principles (§1.3) and acceptance criteria (§13)
- **How did local models actually fail?** List concrete patterns. → weak-model simulator (Phase 1) and guardrails (§8.5)
- **Which safety rules were too strict or too loose?** → tier table (§8.1) and default mode (D-11)
- **What worked well and should be ported?** Tool designs, prompt wording, session handling. → §6, Appendix B, §7
- **Did the extension approach contribute to the problems?** → Lua decision (D-02)
- **Which models and backends were tested?** → G-01

---

## 1. Vision

### 1.1 What we're building

A terminal coding agent that mirrors pi's shape:

- **A tiny core.** A short system prompt and a handful of tools (read, write, edit, bash).
- **A powerful extension system.** Tools, commands, event hooks, UI, providers and session state. Ours is written in **Lua**, embedded via mlua.
- **Self-extension.** The agent can read heimdall's own docs and examples and write extensions for itself.
- **Pi's resource model.** `AGENTS.md` context files, skills, prompt templates, themes and packages.
- **Pi's session model.** JSONL sessions organized as a tree, with branching, compaction, steering and follow-up queues.
- **Pi's modes.** Interactive TUI, print, JSON event stream and RPC.

### 1.2 What's different from pi

| Pi | heimdall |
|---|---|
| No built-in permission system; runs with the user's permissions | A **safety floor** in core: sandboxed bash, automatic checkpoints, a policy engine and hard denies |
| Extensions are trusted, in-process TypeScript | Lua extensions in two tiers: **trusted** or **sandboxed with declared capabilities** |
| Tuned for frontier models | Tuned for **weak local models**: tool-call repair, loop guards, edit assistance, lean context |
| Node.js runtime | One static Rust binary |

### 1.3 Design principles

1. **Reversible means free.** If an action can be undone automatically (a checkpointed file edit, sandboxed bash inside the workspace), allow it without asking. Freedom comes from cheap undo, not from trust.
2. **Irreversible means intent plus gate.** Actions with external or irreversible effects require a stated intent and must pass policy, which may mean asking the user.
3. **The sandbox enforces; classification advises.** Command parsing and risk classification drive the UX: what to ask and how to explain it. The OS sandbox is the actual enforcement. Never rely on parsing for security.
4. **Expect mistakes and make them recoverable.** Every block or error returns a short, instructive message telling the model what happened, why, and what it can do instead.
5. **Extensions can't lower the floor.** Extensions can add restrictions, add tools and change behavior. Only the user's own config can weaken core safety.
6. **Keep the context lean.** Small models have small context windows and weak long-range attention. Every token in the system prompt and tool output must earn its place.
7. **Mimic pi unless there's a reason not to.** Users of pi should feel at home.

### 1.4 Non-goals for v1

- A Windows-native sandbox (**[G-05]**).
- Built-in sub-agents. Pi leaves these to extensions, and so do we. Leave hooks for them (**[G-07]**).
- An extension marketplace or registry (**[G-04]**).
- Frontier-model-specific features such as prompt-cache warming or provider-specific reasoning controls. Add them later as extensions.

---

## 2. Settled decisions

These were settled in design discussion; don't relitigate them without cause.

- **Host language:** Rust (stable), async on tokio.
- **Extension language:** Lua via **mlua**, embedded in-process. Flavor still open: **[D-02]**.
- **Distribution:** a single static binary with docs and examples embedded.
- **Extension authoring has no build step,** like pi's jiti loading: drop in a `.lua` file and reload.
- **The extension API is defined once,** in a schema, and generates bindings, LuaLS stubs and the API reference (§9.4).
- **Tooling for LLM-written extensions:** an `ext_check` tool and a headless `heimdall ext test` runner (§9.5–9.6).
- **Two extension trust tiers,** trusted and sandboxed, with capability manifests (§9.2).

---

## 3. Architecture

### 3.1 Workspace layout (cargo workspace)

```
crates/
  heimdall-ai/        # Provider abstraction, streaming, tool-call parsing & repair, model profiles (≈ pi-ai)
  heimdall-agent/     # Agent loop, turns, events, queues, budgets (≈ pi-agent-core)
  heimdall-tools/     # read/write/edit/bash/grep/find/ls/request_scope
  heimdall-safety/    # Risk tiers, task contract, policy engine, sandbox, checkpoints, guards
  heimdall-session/   # JSONL tree sessions, compaction, checkpoint index
  heimdall-ext-api/   # API schema (api/*.toml) + build.rs codegen
  heimdall-ext/       # mlua host: loading, actors, capability checks, events, UI bridge
  heimdall-tui/       # Terminal UI: renderer, components, editor (≈ pi-tui)
  heimdall-cli/       # Binary: modes (interactive/print/json/rpc), commands, config, resources (≈ pi-coding-agent)
docs/               # User + agent docs (embedded into binary)
examples/extensions # Example extensions (embedded; also used as tests)
extensions-bundled/ # Built-in features written in Lua (dogfooding)
```

### 3.2 Data flow

```
 user input ──► heimdall-cli ──► heimdall-agent loop ──► heimdall-ai (provider stream)
                    │               │   ▲                    │
                    │               │   └── tool results ◄───┤
                    │               ▼                        ▼
                    │        heimdall-safety  ◄── tool call (parsed, repaired)
                    │     (classify → policy → approve? → sandbox/checkpoint)
                    │               │
                    │               ▼
                    │         heimdall-tools ──► filesystem / sandboxed bash
                    │
                    ├──► heimdall-ext (Lua actors) ◄── events from agent/session/tools
                    ├──► heimdall-session (JSONL append, tree, compaction)
                    └──► heimdall-tui (render, dialogs, approvals)
```

### 3.3 Concurrency model

- One tokio multi-threaded runtime for I/O: providers, tools and file operations.
- The TUI runs its render and input loop on a dedicated task or thread, and talks to the agent over channels.
- **Each extension gets its own Lua state on its own dedicated thread, as an actor** (**[D-13]**).
  - Messages in: events and tool invocations. Messages out: results and UI requests.
  - Async host functions (such as `ui.confirm` or `model.complete`) use mlua's async support on a per-actor single-threaded runtime. From Lua they look blocking.
  - Render calls to extension UI components are synchronous requests with a deadline (for example 8ms). On timeout, the last frame is reused and the extension is flagged.
- Per-call instruction limits and per-state memory limits apply to every Lua state, trusted ones included, at more generous values.

---

## 4. Providers and models (`heimdall-ai`)

### 4.1 Provider support (v1)

- **OpenAI-compatible Chat Completions with streaming.** This covers llama.cpp server, Ollama's `/v1` endpoint, LM Studio and vLLM. It's the priority.
- Later, via extension or optional feature: Anthropic Messages and the OpenAI Responses API.
- Custom providers can be registered from Lua (`heimdall.register_provider`), as in pi.

### 4.2 Model profiles (`models.json`)

Small models differ wildly, so each model gets a **profile** describing its quirks:

```json
{
  "models": [{
    "id": "local-coder",
    "provider": "openai-compat",
    "base_url": "http://127.0.0.1:8080/v1",
    "context_window": 32768,
    "max_output": 4096,
    "tool_mode": "native",            // native | text  [D-09]
    "supports_json_schema": true,     // can we constrain tool args?
    "reasoning": { "format": "think_tags", "strip_from_history": true },
    "tokenizer": "endpoint",          // endpoint | estimate  [D-20]
    "temperature": 0.2,
    "max_tools": 8                    // cap active tools for this model
  }]
}
```

### 4.3 Tool-call robustness (key for weak models)

Run this pipeline on every tool call:

1. **Parse.** Take native `tool_calls`, or the text protocol in `text` mode (**[D-09]**).
2. **Repair the JSON leniently.** Fix trailing commas, single quotes, unquoted keys, truncated closing braces and code-fenced JSON. Evaluate existing crates for this (**[G-11]**).
3. **Resolve the tool name.** Match exactly, then by alias table (`read_file`→`read`, `run`/`shell`/`execute`→`bash`, `str_replace`→`edit`), then by edit distance ≤ 2 when unambiguous.
4. **Coerce arguments against the schema.** `"10"`→`10` for integers, a single string into a one-element array, and drop unknown fields with a note.
5. **Validate.** On failure, return a **short** error with one correct example call. Never send a schema dump.
6. **Constrain where possible.** When the backend supports grammars or JSON-schema decoding (llama.cpp grammars, vLLM guided decoding, Ollama `format`), use it for text-mode tool calls.

**Text tool protocol** (**[D-09]**): for models with poor native tool calling. Recommended format, easy for small models and easy to parse:

````
<tool name="read">
{"path": "src/main.rs", "offset": 1, "limit": 200}
</tool>
````

There is one tool call per `<tool>` block. Everything outside the blocks is assistant text.

### 4.4 Context accounting

- Use `usage` from responses when the backend provides it.
- Otherwise use the backend's tokenize endpoint (llama.cpp has one), or estimate at roughly 3.5 characters per token (**[D-20]**).
- Track the context budget per turn. Auto-compaction triggers at a threshold (§7.3).

---

## 5. Agent loop (`heimdall-agent`)

Mirrors pi's lifecycle; see pi's `docs/extensions.md` for the reference flow.

- **A run** starts from user input. It repeats **turns** until the model stops calling tools, or a budget or guard intervenes.
- **A turn** is one model request plus execution of the tool calls it returns. Tool calls in one response may run in parallel. Mutating tools on the same path are serialized.
- **Queues** (pi parity):
  - *Steering* messages are injected before the next turn while the agent is working.
  - *Follow-up* messages are queued until the run settles.
- **Abort:** Esc cancels the in-flight request and tools, and records partial results.
- **Budgets** come from the task contract (§8.2): max turns per run, max tool calls, wall time and max consecutive errors. Hitting one pauses the run and asks the user. In headless modes it stops.
- **Events** are emitted at every stage for extensions (§9.3).

---

## 6. Core tools (`heimdall-tools`)

Pi's default four (read, write, edit, bash), plus helpers aimed at weak models. **[D-17]**: whether grep/find/ls are on by default. Recommendation: on, because structured, read-only tools reduce risky bash usage by small models.

| Tool | Risk tier | Notes for weak models |
|---|---|---|
| `read(path, offset?, limit?)` | R0 | Line-numbered output. Truncation notice says exactly how to continue ("call read with offset=201"). Images later. |
| `write(path, content, intent?)` | R1 in workspace | Checkpoints before writing. Refuses to overwrite a file not read this session unless `force: true` (catches hallucinated overwrites). |
| `edit(path, old_text, new_text, intent?)` | R1 in workspace | Exact match → whitespace-normalized match → on failure, return the **closest candidate region with line numbers** and a similarity score. Never auto-apply fuzzy matches in v1 (**[D-10]**). Multiple matches → error listing line numbers. |
| `bash(command, timeout?, intent?)` | Classified (§8.3) | Runs in the sandbox. Output is truncated head+tail with a notice. Non-zero exit status is reported clearly. |
| `grep(pattern, path?, glob?)` | R0 | Uses ripgrep's crates (`grep`, `ignore`). Respects .gitignore. Result count is capped. |
| `find(glob, path?)` | R0 | Respects .gitignore. Capped. |
| `ls(path?)` | R0 | Capped, sorted, marks directories. |
| `request_scope(capability, reason)` | — | Asks the user to expand the task contract (§8.2). Returns granted, denied, or granted with a narrowed scope. |

**Intent parameter (**[D-19]**):** `intent` (one short sentence) is optional on write, edit and bash. It is **required when the action classifies as R2 or higher**. If it's missing, the call returns: "This action needs an `intent`: one sentence explaining why. Retry with intent." It is shown in approval dialogs and logged.

**User bash** (pi parity): `!cmd` in the editor runs a command as the user. It goes through policy, but at user-level trust, and its output is optionally added to the context.

---

## 7. Sessions (`heimdall-session`)

### 7.1 Format (pi-like)

- One JSONL file per session under `~/.heimdall/sessions/<cwd-slug>/<timestamp>_<id>.jsonl`.
- Every entry has `id`, `parent_id`, `type` and `ts`, which makes the session a **tree**.
- Entry types:
  - Conversation: `message` (user/assistant/system), `tool_result`.
  - Extension data: `custom` (not sent to the model), `custom_message` (sent to the model).
  - Maintenance: `compaction`, `label`.
  - Heimdall-specific: `checkpoint`, `policy_decision`, `contract_change`.
- Commands: `/new`, `/resume`, `/tree` (navigate and branch), `/fork`, `/label`, `/export` (HTML later).

### 7.2 Checkpoints and undo (heimdall-specific)

**[D-05]**: the mechanism. Recommendation: a **shadow git repository** whose `GIT_DIR` lives in `~/.heimdall/checkpoints/<cwd-slug>/`, with the user's workspace as its work tree.

- Snapshot at the start of every turn and before every bash call classified R1 or higher.
- It respects `.gitignore` and has a size cap, skipping large binaries with a warning.
- Each snapshot is recorded as a `checkpoint` entry.
- **[D-06]**: shell out to the `git` CLI (simple, requires git) or use `gix` (pure Rust, more work). Recommendation: `git` CLI for v1.
- Commands:
  - `/undo` restores files to the checkpoint before the last turn.
  - `/rewind` picks a tree node and restores both the conversation branch and the files.
  - Restores themselves get a checkpoint first, so they can be undone too.
- Writes outside the workspace aren't covered by the shadow repo. That's one reason they're R2 or higher.

### 7.3 Compaction

- Triggers at **[D-16]** (recommendation: 70% of the context window), or manually with `/compact [focus]`.
- Small models summarize poorly, so use a **structured template**: goal, constraints, decisions made, files touched with a one-line purpose each, current state, open problems, next step.
- The last N turns are kept verbatim. The task contract and file-change list are always preserved.
- An optional `compaction_model` (a stronger or faster model) can be configured.
- Extensions can override the summary via the `session_before_compact` event.

---

## 8. Safety system: "freedom with intent" (`heimdall-safety`)

### 8.1 Risk tiers

| Tier | Meaning | Examples | Default (guarded mode) |
|---|---|---|---|
| **R0 Observe** | No side effects | read/grep/find/ls in the workspace; `git status`/`diff`/`log` | Allow |
| **R1 Reversible-local** | Side effects confined to the workspace and checkpointed | write/edit in the workspace; sandboxed bash with workspace-write and no network | Allow + checkpoint |
| **R2 Scope-expanding** | Reversible with effort, or leaves the contract | network access, package installs, writes outside the workspace, long-running/background processes, reading secret-pattern files (**[D-12]**) | Needs intent; ask unless granted in the contract |
| **R3 Irreversible/external** | Can't be undone locally | `git push`, publishing, deleting outside the workspace, `sudo`, sending data to external services, unsandboxed execution | Needs intent; always ask (unless the contract has an explicit standing grant) |
| **Forbidden** | Hard deny | Disabling the sandbox from within a session; reading `~/.ssh/id_*`; writing heimdall's own config or policy files | Deny (only config edited by the user can change these) |

**Modes** (**[D-11]**), switchable with `/mode`:

- `strict`: ask for R1 and above.
- `guarded` (default): ask for R2 and above unless granted.
- `free`: R0–R2 automatic within the sandbox and checkpoints; ask for R3.

Every mode keeps the sandbox, checkpoints and forbidden list.

### 8.2 The task contract (the "intent" scope)

Each session has a **contract** that is visible in the status bar and editable with `/contract` and `/grant`:

```json
{
  "workspace": "/home/me/proj",
  "extra_paths": { "read": ["~/notes"], "write": [] },
  "network": "off",                 // off | on   ([G-03] for domain allowlists)
  "grants": ["pkg-install"],        // standing R2/R3 categories the user pre-approved
  "budgets": { "max_turns": 40, "max_tool_calls": 150, "max_consecutive_errors": 4, "wall_minutes": 30 }
}
```

- Defaults come from global settings, then project `.heimdall/settings.json`, then CLI flags.
- **`request_scope`** lets the model ask for an expansion. The approval card shows the reason, and the user picks:
  - once,
  - for the session (adds a grant),
  - deny with a message back to the model.
- This turns surprise denials into explicit, explainable requests, which small models handle well.

### 8.3 Bash classification (advisory) and sandbox (enforcement)

**Classification:**
- Parse with tree-sitter-bash. Extract each command, its arguments, redirects, pipes and subshells.
- Match against rule tables (allow/ask/deny patterns plus tier assignments).
- If the parse fails, or the command uses `eval`, `bash -c` with a dynamic string, curl-piped-to-shell and similar patterns, it's at least R2.

**Sandbox** (**[D-07]**, **[D-08]**):

- **Linux (v1):**
  - Landlock limits filesystem access: read everywhere except deny paths; write only to the workspace, temp directory and granted paths.
  - seccomp blocks network unless the contract enables it.
  - Fallback: bubblewrap, if installed and Landlock is unavailable (old kernels).
  - Codex CLI uses a similar Landlock + seccomp approach; study it.
- **macOS (phase 8):** a Seatbelt (`sandbox-exec`) profile generated from the contract.
- **Windows:** WSL only for v1 (**[G-05]**).
- Unsandboxed execution is always R3.

**Blocked-action messages** must be instructive:

```
BLOCKED (R2: network access). The task contract has network=off.
If you need network access, call request_scope(capability="network", reason="...").
Otherwise, continue without it.
```

### 8.4 Policy engine

Evaluation order for every tool call:

1. **Forbidden list** (core, user config only).
2. **Classification** → tier.
3. **Contract grants** check.
4. **Extension `tool_call` hooks** (pi parity). Hooks can block, ask, modify input or add notes. They **cannot** downgrade a tier or bypass the forbidden list or sandbox.
5. **Mode rules** decide allow, ask or deny.
6. On allow: checkpoint if R1 or higher, then execute in the sandbox.

Failure handling:
- **Fail closed.** If a hook errors, or a decision can't be made, the call is treated as ask in interactive mode and deny in headless modes.
- Every decision is written as a `policy_decision` session entry: tool, args summary, tier, intent, decision, decider.

### 8.5 Guardrails for weak models

- **Loop detection.** The same tool name and argument hash appearing 3× in a 10-call window triggers an injected nudge ("You've called X with the same arguments 3 times; the result won't change. Try a different approach or ask the user."). At 5×, pause and ask the user.
- **Error streaks.** After `max_consecutive_errors`, inject a nudge to re-read the relevant files and re-plan. After 2× that, pause.
- **No-progress heuristic.** N turns with no file change, no new file read and no user-facing answer triggers a nudge (**[G-12]**: tune N).
- **Edit assistance** (§6): return candidate regions instead of a bare "not found."
- **Output hygiene.** Truncate tool output with head and tail kept. Strip ANSI codes. Collapse repetitive lines ("… 240 similar lines omitted").
- **Post-edit verification** (bundled extension): optionally run the project's check command (`cargo check`, `npm test`, …) after edits and feed back a short summary.
- **Read-before-write.** Refuse `write` over an existing file the model hasn't read this session, unless `force: true`.

### 8.6 Approval UX

- **Interactive:** a modal card showing the tool, a compact argument summary, the tier, the intent and the classification reason.
  - Keys: `y` once, `s` session grant, `n` deny, `e` deny with a message.
  - Policy-driven, so extensions can add context lines.
- **Print/JSON modes:** no prompts. Use flags such as `--allow network,pkg-install` and `--mode free`. Anything else above the threshold is denied, and the denial is reported to the model (**[D-18]**).
- **RPC mode:** approval requests are forwarded to the client.

### 8.7 Prompt injection (**[G-09]**)

File contents, command output and web pages can contain instructions. In v1, tiers and the sandbox bound the damage. Gaps to design:

- Marking tool output as untrusted data in the prompt.
- A user-visible indicator when an R2+ request follows closely after reading external content.

---

## 9. Extension system (`heimdall-ext`, `heimdall-ext-api`)

### 9.1 Runtime choice (**[D-02]**)

| Option | Pros | Cons |
|---|---|---|
| **Lua 5.4** (recommended) | Best LLM familiarity among the options; LuaLS annotations give typed stubs; integers; mature | No static types beyond annotations |
| **Luau** | Real gradual types plus a built-in sandbox mode | Less LLM training data; tooling differs (luau-lsp); not standard Lua |
| **LuaJIT** | Fastest | Lua 5.1 semantics; confuses models that mix versions |

Whatever you pick, state the version prominently in every doc, because models mix up 5.1 and 5.4 semantics. Also verify mlua's support for memory limits and instruction hooks on that flavor before committing.

### 9.2 Loading, manifests and trust tiers

**Locations** (pi parity):
- `~/.heimdall/extensions/` (global).
- `<project>/.heimdall/extensions/` (project; requires project trust, see below).
- `--extension <path>` for development.

**Shape:** a single file `name.lua`, or a directory containing `init.lua`.

**The manifest lives in the extension itself.** The file returns a table:

```lua
return {
  name = "git-guard",
  version = "0.1.0",
  description = "Asks before git push and blocks force-push to main",
  tier = "sandboxed",                      -- "sandboxed" | "trusted"
  capabilities = { "events.tool_call", "ui" },
  setup = function(heimdall)
    heimdall.on("tool_call", function(ev, ctx)
      -- ...
    end)
  end,
}
```

**Tiers:**
- **Sandboxed** (the default for anything not authored by the user):
  - Only the declared capabilities are available. Calls outside them raise a clear error.
  - No raw `io`, `os` or `require` of arbitrary files.
  - Tight memory and instruction limits.
- **Trusted:** all capabilities, including `fs`, `process` and `net` host APIs. Raw `io` and `os` stay off; extensions use host APIs so that policy still sees their effects (**[G-13]**).

**Approval:**
- New or changed extensions show their manifest for approval: tier plus capabilities.
- Changing capabilities requires re-approval.
- **Project trust** (pi parity): project-local extensions and settings require a one-time per-project trust prompt.

**Extension tools pass through the same policy.**
- A registered tool declares `tier` (R0–R3) and annotations: `read_only`, `destructive`, `open_world`, mirroring the MCP-style hints pi uses.
- A sandboxed extension cannot declare a tier lower than its capabilities imply. For example, a tool from an extension with `net` is at least R2.

### 9.3 API surface (pi parity target)

| Pi | heimdall (Lua) | Phase |
|---|---|---|
| `pi.on(event, handler)` | `heimdall.on(event, fn(ev, ctx))` | 5 |
| `pi.registerTool` | `heimdall.register_tool{ name, description, params, tier, annotations, execute }` | 5 |
| `pi.registerCommand` | `heimdall.register_command(name, { description, handler })` | 5 |
| `pi.registerShortcut` / `registerFlag` | `heimdall.register_shortcut` / `register_flag` | 5 |
| `pi.sendUserMessage` / `sendMessage` | `heimdall.send_user_message` / `heimdall.send_message` | 5 |
| `pi.appendEntry` | `heimdall.append_entry` | 5 |
| `pi.setActiveTools` / `getActiveTools` | `heimdall.set_active_tools` / `get_active_tools` | 5 |
| `pi.registerProvider` | `heimdall.register_provider` | 7 |
| `pi.registerToolRenderer` | `heimdall.register_tool_renderer` | 6 |
| `pi.events` (inter-extension bus) | `heimdall.events.emit/on` | 6 |
| `ctx.ui.*` | `ctx.ui.notify/confirm/select/input/status/widget/custom` | 5–6 |
| `ctx.executeTool` | `ctx.execute_tool(name, args)` (goes through policy) | 6 |
| `pi.registerMcpServer` | **[G-08]** | later |

**Events** (start with this subset, using pi's names where they apply):
- Input: `input`, `before_agent_start`.
- Runs and turns: `agent_start`, `agent_end`, `turn_start`, `turn_end`, `context`.
- Messages: `message_start`, `message_update`, `message_end`.
- Tools: `tool_call` (gate), `tool_execution_update`, `tool_result` (chain).
- Session: `session_start`, `session_shutdown`, `session_before_compact`.
- Heimdall-specific: `policy_decision` (notification), `scope_request` (gate).

**Composition semantics** (pi parity): each event declares one kind:
- **notification:** return values are ignored.
- **mutation:** later handlers see in-place changes.
- **chain:** each handler receives the previous handler's return value.
- **gate:** the first block wins and the operation stops.

Handler errors are reported and never crash the host. A failing `tool_call` gate fails closed.

### 9.4 Single-source API definition → bindings, stubs and docs

**Goal:** the Lua API is defined **once**. Codegen produces the Rust host trait and mlua glue, the LuaLS stubs the model reads, and the markdown API reference. They can never drift apart.

**[D-14]**: schema format and codegen. Recommendation: TOML files under `crates/heimdall-ext-api/api/`, processed by `build.rs`.

```toml
# api/ui.toml
[[function]]
name = "ui.confirm"
doc = "Ask the user a yes/no question. Returns false in headless modes."
capability = "ui"
async = true
params = [
  { name = "title",   type = "string" },
  { name = "message", type = "string" },
]
returns = "boolean"

[[type]]
name = "ToolCallEvent"
doc = "Emitted before a tool executes. Return {block=true, reason=...} to block."
fields = [
  { name = "tool",   type = "string" },
  { name = "input",  type = "table" },
  { name = "tier",   type = "\"R0\"|\"R1\"|\"R2\"|\"R3\"" },
  { name = "intent", type = "string?" },
]

[[event]]
name = "tool_call"
kind = "gate"
payload = "ToolCallEvent"
result = "ToolCallResult?"
capability = "events.tool_call"
```

**Type grammar:** `string | integer | number | boolean | table | T? | T[] | {K: V} | "lit"|"lit" | function(args): ret`, plus named types.

**Generated artifacts:**

1. **`ExtHost` Rust trait.** One method per function with typed arguments. The host implements it, and the compiler forces the implementation to stay complete.
2. **mlua registration glue.** Builds the `heimdall` global table, checks capabilities on each call, and converts arguments with precise errors. Example: `heimdall.ui.confirm: argument 2 'message' expected string, got nil`.
3. **LuaLS stub** at `docs/api/heimdall.lua` (`---@meta`, `---@class`, `---@param`, `---@return`, `---@alias` for events). Plus `.d.luau` output if **[D-02]** = Luau.
4. **Markdown reference** at `docs/api.md`, grouped by capability, including event-kind semantics.
5. **Signature table** (JSON) consumed by `ext_check` (§9.5).

Add a CI check that regenerates and diffs these artifacts, failing on drift.

Example of generated stub output:

```lua
---@meta
---@class heimdall.ui
local ui = {}

---Ask the user a yes/no question. Returns false in headless modes.
---Requires capability: ui
---@param title string
---@param message string
---@return boolean
function ui.confirm(title, message) end
```

### 9.5 `ext_check`: static and dry-load validation

Available as an **agent tool** (R0) and as a CLI command, `heimdall ext check <path>`.

1. **Manifest.** The file returns a table with the required fields; capabilities are known names; the tier is valid.
2. **Syntax.** Compile the chunk without executing it, and report line and column.
3. **Static checks** (**[D-15]**). Recommendation: a custom pass using the `full_moon` parser, checking:
   - Unknown `heimdall.*` and `ctx.*` members, against the signature table.
   - Argument count against the signature.
   - Literal type mismatches.
   - Accidental globals (assignment without `local`).
   - Common Lua pitfalls: `!=`, `continue` in 5.4, `#` on sparse tables in obvious cases, `string.match` used with regex syntax.
   - Optionally, also run `lua-language-server --check` when it's installed.
4. **Dry load.** Run `setup(heimdall)` against a **recording host** that has no side effects. Capture registrations (tools, commands, events). Validate tool parameter schemas and event names. Errors include a traceback.
5. **Output** is compact structured diagnostics, which small models handle better than prose:

```json
{"ok": false, "diagnostics": [
  {"file":"guard.lua","line":12,"col":9,"severity":"error",
   "msg":"unknown member heimdall.ui.confirmm","hint":"did you mean heimdall.ui.confirm?"}
], "registered": {"tools":["safe_push"],"commands":[],"events":["tool_call"]}}
```

### 9.6 `heimdall ext test`: headless extension tests

- Test files named `*_test.lua` live next to the extension.
- Runnable via the CLI, or the `ext_test` agent tool (R1: it runs code, but in a fake host).
- Test API (generated stubs cover it too):

```lua
local t = require("heimdall.test")

t.case("blocks force push to main", function(h)
  h.load("./git-guard.lua")
  local r = h.emit("tool_call", {
    tool = "bash", tier = "R3",
    input = { command = "git push --force origin main" },
    intent = "publish fix",
  })
  h.expect(r).to_match({ block = true })
end)

t.case("asks before normal push", function(h)
  h.load("./git-guard.lua")
  h.ui.on_confirm(function(title, msg) return true end)  -- scripted user
  local r = h.emit("tool_call", { tool = "bash", tier = "R3",
    input = { command = "git push origin feature" } })
  h.expect(r).to_be_nil()
  h.expect(h.ui.calls("confirm")).to_equal(1)
end)
```

- **The fake host provides:**
  - Scripted UI answers.
  - A mock model (`h.model.reply("...")`) for extensions that make nested model calls.
  - A temporary-directory filesystem.
  - A virtual clock.
  - Captured notifications and entries.
- **Output** is structured JSON (pass/fail per case, assertion diffs, tracebacks), so the agent can iterate.

### 9.7 Extension UI

- **Primitives first:** `notify`, `confirm`, `select`, `input`, `status(key, text)`, `widget(slot, lines)`. These are enough for most extensions, and they work in RPC mode.
- **Custom components** (pi-tui parity): `ctx.ui.custom(component)`, where:
  - the component is `{ render = function(width) return lines end, handle_input = function(key) ... end, dispose = ... }`;
  - lines are strings that may be styled with `heimdall.style.*` helpers, which emit ANSI;
  - render runs under a deadline and an instruction cap (§3.3).
- Custom renderers for tool calls and results arrive in phase 6.

### 9.8 Bundled extensions (dogfooding)

Ship these as real Lua extensions. They serve as working features and as the agent's best examples:

- `permission-presets`: pre-defined policy rule sets, such as "git safety" and "no package installs."
- `post-edit-check`: runs a configured check command after edits (§8.5).
- `todo`: a branch-aware todo tool that rebuilds its state from session entries.
- `plan-mode`: a read-only exploration mode that proposes a numbered plan.
- `git-guard`: the example from §9.6.

---

## 10. Self-extension: guiding the LLM

### 10.1 Docs map in the system prompt (pi parity)

Pi's prompt points the model at its installed docs and examples, and tells it to read them completely before working on pi topics. Do the same:

- On first run (and on version change), extract the embedded `docs/` and `examples/` to `~/.heimdall/docs/<version>/`.
- Add a short section to the system prompt:

```
Heimdall documentation (read only when the user asks about heimdall itself, extensions, skills, themes, or the TUI):
- Docs: ~/.heimdall/docs/<v>/README.md and ~/.heimdall/docs/<v>/docs/
- API stubs (exact signatures): ~/.heimdall/docs/<v>/docs/api/heimdall.lua
- Examples: ~/.heimdall/docs/<v>/examples/extensions/ and bundled extensions
When writing an extension: read docs/extensions.md fully, start from the smallest matching example,
then run ext_check, then write a *_test.lua and run ext_test until it passes.
```

### 10.2 Docs set (write for agents and humans)

- `docs/extensions.md`: lifecycle, a "choose an event by intent" table (pi style), tiers and capabilities, and patterns.
- `docs/lua-pitfalls.md`: the chosen Lua version, 1-based indexing, `~=`, no `continue`, Lua patterns versus regex (use `heimdall.re`), `#` with nil holes, `:` versus `.`, integer division.
- `docs/safety.md`: tiers, the contract, how to declare tool tiers and how to request scope.
- `docs/tui.md`, `docs/skills.md`, `docs/settings.md`, `docs/api.md` (generated).
- A **`writing-extensions` skill** that loads on demand (progressive disclosure, so the base prompt stays small).

### 10.3 Host helpers (so models don't hand-roll them)

`heimdall.json`, `heimdall.re` (Rust regex), `heimdall.path`, `heimdall.fs` (capability-gated, policy-checked), `heimdall.http` (capability `net`, R2 by default), `heimdall.process` (trusted only, policy-checked), `heimdall.style`, `heimdall.log`.

---

## 11. Interactive TUI (`heimdall-tui`)

**[D-03]**: rendering approach.

- **Recommended: a custom line-based differential renderer on crossterm, inline** (no alternate screen), similar to how pi-tui renders.
  - It keeps native scrollback and text selection.
  - It matches the `render(width) → lines` component model used for extensions.
- **Alternative: ratatui.** Faster to build, but its inline-viewport mode is limited. Full-screen ratatui loses the pi feel.

Must-haves:
- A multi-line editor with history.
- `@` file references with fuzzy completion.
- `!cmd` user bash.
- Slash-command completion.
- Streaming markdown rendering, with code highlighting later.
- Collapsible tool calls and results.
- A status bar showing model, context usage, mode, contract summary, active extensions and the checkpoint count.
- The approval card (§8.6).
- `/tree` navigator.
- Steering and follow-up input while the agent runs.
- Keybindings in `keybindings.json`; themes in `themes/*.json` (pi parity).

---

## 12. Configuration and resources

**[D-04]**: config format. Recommendation: JSON, for pi parity and because LLMs edit it reliably. The exception is the Lua manifests, which live inside the extensions.

```
~/.heimdall/
  settings.json        # defaults: model, mode, contract defaults, budgets, UI prefs
  models.json          # model profiles (§4.2)
  keybindings.json
  AGENTS.md            # global instructions
  extensions/  skills/  prompts/  themes/  packages/
  sessions/  checkpoints/  docs/<version>/  logs/
<project>/
  AGENTS.md            # project context (also read CLAUDE.md for compatibility)
  .heimdall/settings.json  extensions/  skills/  prompts/
```

- **Skills** (pi parity): `SKILL.md` with name and description frontmatter. Only the descriptions go in the prompt; the model reads the full file when it's relevant.
- **Prompt templates:** `prompts/name.md` → `/name args`.
- **Packages** (phase 7, **[G-04]**): `heimdall install git:<url>` or a local path. A package manifest lists extensions, skills, prompts and themes. Installation shows the aggregate capability manifest for approval.

---

## 13. Roadmap

Each phase ends with its acceptance criteria passing in CI, where automatable.

**Phase 0: skeleton.**
- Workspace and crates, CI (fmt, clippy, tests), the `heimdall --version` binary, tracing and logging.

**Phase 1: providers (`heimdall-ai`).**
- An OpenAI-compatible streaming client, model profiles, native and text tool modes, the repair pipeline, a mock provider and a *weak-model simulator* mock (emits malformed JSON, wrong tool names, repeated calls).
- ✅ The repair pipeline passes a fixture suite of 50+ malformed tool calls.
- ✅ `heimdall -p "hi"` streams from a local llama.cpp or Ollama server.

**Phase 2: agent loop, core tools and sessions** (print and JSON modes).
- Turns, queues, abort, budgets; read/write/edit/bash (unsandboxed for now, behind a dev flag)/grep/find/ls; JSONL tree sessions; `--resume`.
- ✅ A mock-provider end-to-end test edits a file correctly.
- ✅ The session file round-trips.
- ✅ Edit returns candidate regions on a miss.

**Phase 3: safety core.**
- Tiers, bash classification, the contract, the policy engine, the Linux sandbox, shadow-git checkpoints with `/undo`, loop and error guards, instructive block messages, headless approval flags, policy-decision logging.
- ✅ A sandbox test suite: writes outside the workspace fail, network is blocked when off, escape attempts fail.
- ✅ Every simulator failure mode triggers the correct guard.
- ✅ `/undo` restores files after both edits and bash changes.

**Phase 4: interactive TUI.**
- Renderer, editor, streaming output, tool display, approval card, status bar, slash commands, steering and follow-up, `/tree`, `/mode`, `/grant`, `/contract`.
- ✅ A manual QA checklist.
- ✅ Render benchmarks: no visible flicker; 10k-line sessions scroll smoothly.

**Phase 5: extension runtime.**
- The API schema and codegen (§9.4), Lua actors, manifests, tiers, capabilities, the event subset, tools and commands, UI primitives, `/reload`, project trust.
- ✅ Generated stubs, glue and docs are drift-free in CI.
- ✅ A sandboxed extension cannot exceed its capabilities.
- ✅ A runaway loop is killed without freezing the UI.

**Phase 6: self-extension tooling.**
- Embedded docs and extraction, the docs map in the prompt, `ext_check`, `ext test`, the `writing-extensions` skill, bundled extensions, custom UI components, tool renderers, the inter-extension event bus.
- ✅ Given "write an extension that asks before git push," a reference local model (**[G-01]**) produces an extension that passes `ext_check` and its own tests within N attempts.

**Phase 7: resources and packages.**
- Skills, prompt templates, themes, keybindings, packages, `register_provider`.

**Phase 8: polish and portability.**
- macOS Seatbelt sandbox, RPC mode, HTML export, compaction tuning, the weak-model evaluation suite in CI (nightly), MCP (**[G-08]**).

---

## 14. Testing strategy

- **Unit and fixture tests:**
  - The repair pipeline.
  - Bash classification (a large table of commands → tier).
  - The edit matcher.
  - Session tree operations.
- **Weak-model simulator:**
  - A scripted mock provider that reproduces typical small-model failures: malformed JSON, invented tools, edits against stale content, loops, ignoring block messages.
  - It makes guardrail tests deterministic.
- **Sandbox tests:** an escape-attempt suite on Linux (and macOS later), run in CI containers with Landlock available (**[G-14]**: CI kernel support).
- **Real-model evaluation** (**[G-01]**, **[G-02]**):
  - A small task suite run nightly against 2–3 reference local models.
  - Metrics: task success, policy violations attempted, blocks recovered from, loops triggered, average turns.
  - Use it to tune guards and prompts.
- **Extension tests:** every bundled extension and example has `*_test.lua`, run in CI with `heimdall ext test`.

---

## 15. Decision and gap index

| ID | Topic | Recommendation / status |
|---|---|---|
| D-01 | Project name | **Decided:** Heimdall v2; binary `heimdall` |
| D-02 | Lua flavor | Lua 5.4 (verify mlua memory and hook support) |
| D-03 | TUI rendering | Custom inline differential renderer on crossterm |
| D-04 | Config format | JSON |
| D-05 | Checkpoint mechanism | Shadow git repo outside the workspace |
| D-06 | Git implementation | `git` CLI for v1 |
| D-07 | Sandbox platforms | Linux v1, macOS phase 8, Windows via WSL |
| D-08 | Linux sandbox implementation | Landlock + seccomp; bubblewrap fallback |
| D-09 | Tool-call mode / text protocol | Native first; `<tool name>` JSON-block fallback |
| D-10 | Fuzzy edit auto-apply | Never in v1; return candidates |
| D-11 | Default mode and tier mapping | `guarded` as in §8.1 |
| D-12 | Secret-pattern reads in the workspace | R2 (ask) |
| D-13 | Lua threading | One state per extension, actor on a dedicated thread |
| D-14 | API schema format | TOML + build.rs codegen |
| D-15 | ext_check static depth | Custom `full_moon` pass + optional LuaLS |
| D-16 | Compaction threshold and model | 70%; optional compaction model |
| D-17 | grep/find/ls on by default | Yes |
| D-18 | Headless approvals | Fail closed; `--allow` flags |
| D-19 | Intent parameter | Optional; required for R2+ |
| D-20 | Token counting | Response usage → tokenize endpoint → estimate |
| G-01 | Reference local models and backends | **Choose 2–3 models across sizes** |
| G-02 | Evaluation task suite | Design 20–30 representative tasks |
| G-03 | Network domain allowlists | v1 is on/off; allowlists need a proxy |
| G-04 | Package format and registry | Design in phase 7 |
| G-05 | Windows support | Out of v1 |
| G-07 | Sub-agents | Extension-level; design host hooks |
| G-08 | MCP support | Pi has it built in; plan for phase 8 |
| G-09 | Prompt-injection mitigations | Design beyond tiers |
| G-11 | JSON repair crate | Evaluate or write our own |
| G-12 | No-progress heuristic tuning | Calibrate using the eval suite |
| G-13 | Trusted-tier raw `io`/`os` | Off by default? Confirm |
| G-14 | CI kernel support for Landlock | Verify the runner |
| G-15 | Lessons from Heimdall v1 | Gather from the v1 project (§0) |

## Appendix A: candidate crates (verify current versions and maintenance)

`tokio`, `reqwest` (rustls), `serde`/`serde_json`, `mlua` (features: chosen Lua, `vendored`, `async`, `serialize`), `crossterm`, `ratatui` (only if D-03 changes), `tree-sitter` + `tree-sitter-bash`, `landlock`, `seccompiler` or `libseccomp`, `grep` + `ignore` (ripgrep), `similar` (diffs), `full_moon` (Lua parsing), `clap`, `thiserror`/`anyhow`, `tracing`, `unicode-width`, `syntect` (later), `gix` (if D-06 changes).

## Appendix B: core system prompt draft (keep under ~600 tokens)

```
You are a coding agent running inside heimdall in the user's terminal. You can read files,
write and edit files, and run shell commands to complete the user's task.

Tools: {tool_list}

How to work:
- Read before you edit. Make small, targeted edits. After changes, verify (run tests or the build).
- Your changes inside the workspace are checkpointed and can be undone, so act confidently there.
- Some actions need permission (network, installs, files outside the workspace, git push, etc.).
  For those, include a one-sentence `intent`. If something is BLOCKED, read the reason:
  use request_scope to ask for access, or continue another way. Do not retry the same blocked action.
- If you are stuck or unsure what the user wants, ask a short question instead of guessing.
- Be concise in your replies.

Task contract: workspace={workspace}, network={network}, grants={grants}

{docs_map_section}
{skills_descriptions}
{project_context}
Current working directory: {cwd}
```
