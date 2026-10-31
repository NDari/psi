# Psi (ψ) — Implementation Plan

> A pi-style coding agent harness in **Rust + mlua**, designed for **local, under-powered LLMs** that will make mistakes.
> Psi is the successor to Heimdall v1, which was written in Go. The executable is `psi`.
> Goal: give the model the freedom to do almost anything, **with intent** — not pi's "wild west," and not a cage.

**How to use this document (for you and Claude Code)**

- This is the source of truth for the project. Work phase by phase (§13). Each phase has acceptance criteria; don't start the next phase until they pass.
- **All design decisions are made.** They are referenced as `D-xx` and logged with their rationale in §15. To change one, add a "Superseded" note to its log entry explaining why. Don't silently diverge.
- Items marked **`[G-xx]`** are *gaps*: things still to research or design. They are indexed in §16. Resolve them in the phase that needs them and record the outcome there.
- "Pi parity" notes show where we deliberately mimic pi. Pi's source (`earendil-works/pi`, MIT) is the behavioral reference. Read it, but don't copy prompts or docs verbatim; write our own.
- Naming: the project is **Psi** (ψ). The binary is `psi`, crates are `psi-*`, the config directory is `~/.psi/`, and the Lua global is `psi` (D-01).

---

## 0. Lessons from Heimdall v1 (Go) — **[G-15]** to fill in

Gather these from the v1 project, citing specific files, commits or issues for each. Tag each lesson with the section it affects, and update that section.

- **What fell short of the goal?** → principles (§1.3) and acceptance criteria (§13)
- **How did local models actually fail?** List concrete patterns. → weak-model simulator (Phase 1) and guardrails (§8.6)
- **Which safety rules were too strict or too loose?** → tier table (§8.1) and default mode (D-11)
- **What worked well and should be ported?** Tool designs, prompt wording, session handling. → §6, Appendix B, §7
- **Did the extension approach contribute to the problems?** → extension runtime (§9)
- **Which models and backends were tested?** → G-01
- **What did v1's bubblewrap and Seatbelt setup get right or wrong?** Arguments, profiles, AppArmor issues, tools that broke. → §8.4
- **What did v1's mvdan/sh-based bash rules allow and deny?** Port the rule lists into the bash rule table. → §8.3

---

## 1. Vision

### 1.1 What we're building

A terminal coding agent that mirrors pi's shape:

- **A tiny core.** A short system prompt and a handful of tools (read, write, edit, bash).
- **A powerful extension system.** Tools, commands, event hooks, UI, providers and session state, written in **Lua 5.4** and embedded via mlua.
- **Self-extension.** The agent can read psi's own docs and examples and write extensions for itself.
- **Pi's resource model.** `AGENTS.md` context files, skills, prompt templates, themes and packages.
- **Pi's session model.** JSONL sessions organized as a tree, with branching, compaction, steering and follow-up queues.
- **Pi's modes.** Interactive TUI, print, JSON event stream and RPC.

### 1.2 What's different from pi

| Pi | Psi |
|---|---|
| No built-in permission system; runs with the user's permissions | A **safety floor** in core: sandboxed bash, a filtering network proxy, automatic checkpoints, a policy engine and hard denies |
| Extensions are trusted, in-process TypeScript | Lua extensions in two tiers: **trusted** or **sandboxed with declared capabilities** |
| Tuned for frontier models | Tuned for **weak local models**: tool-call repair, loop guards, edit assistance, lean context |
| Node.js runtime | One static Rust binary |

### 1.3 Design principles

1. **Reversible means free.** If an action can be undone automatically (a checkpointed file edit, sandboxed bash inside the workspace), allow it without asking. Freedom comes from cheap undo, not from trust.
2. **Irreversible means intent plus gate.** Actions with external or irreversible effects require a stated intent and must pass policy, which may mean asking the user.
3. **The sandbox enforces; classification advises.** Command parsing and risk classification drive the UX: what to ask and how to explain it. The OS sandbox and network proxy are the actual enforcement. A classifier bug must never become a security hole.
4. **Expect mistakes and make them recoverable.** Every block or error returns a short, instructive message telling the model what happened, why, and what it can do instead.
5. **Safety lives in core, and extensions can't lower the floor.** Extensions can add restrictions, add tools and change behavior. Only the user's own config can weaken core safety.
6. **Keep the context lean.** Small models have small context windows and weak long-range attention. Every token in the system prompt and tool output must earn its place.
7. **Mimic pi unless there's a reason not to.** Users of pi should feel at home.

### 1.4 Non-goals for the first release

- A Windows-native sandbox (**[G-05]**).
- Built-in sub-agents. Pi leaves these to extensions, and so do we. Leave hooks for them (**[G-07]**).
- An extension marketplace or registry (**[G-04]**).
- Frontier-model-specific features such as prompt-cache warming or provider-specific reasoning controls. Add them later as extensions.

---

## 2. Decisions at a glance

Full rationale is in §15.

| Area | Decision |
|---|---|
| Host | Rust (stable), async on tokio, one static binary with docs and examples embedded |
| Extension language | Lua 5.4 via mlua, in-process, no build step (D-02) |
| Extension API definition | TOML schema + `build.rs` codegen → Rust trait, mlua glue, LuaLS stubs, docs, `ext_check` table (D-14) |
| Extension threading | One Lua state per extension, actor on a dedicated thread (D-13) |
| Extension tooling | `ext_check` (custom `full_moon` pass + optional LuaLS) and headless `psi ext test` (D-15) |
| TUI | Custom inline line-based differential renderer on crossterm (D-03) |
| Config | JSON with comments (JSONC) (D-04) |
| Checkpoints | Shadow git repo outside the workspace, via the `git` CLI (D-05, D-06) |
| Sandbox | bubblewrap (Linux) + Seatbelt (macOS), both from the start; Windows via WSL (D-07, D-08) |
| Network | Built-in HTTP CONNECT + SOCKS5 proxy with per-domain allowlists and live approvals (D-08) |
| Bash classification | tree-sitter-bash; every simple command checked; data-driven rule tables (D-21) |
| Default safety mode | `guarded` (D-11) |
| Tool calls | Per-model profile, native by default, text-protocol fallback (D-09) |
| Edits | Exact and whitespace-normalized matches apply; fuzzy matches never auto-apply (D-10) |

---

## 3. Architecture

### 3.1 Workspace layout (cargo workspace)

```
crates/
  psi-ai/        # Provider abstraction, streaming, tool-call parsing & repair, model profiles (≈ pi-ai)
  psi-agent/     # Agent loop, turns, events, queues, budgets (≈ pi-agent-core)
  psi-tools/     # read/write/edit/bash/grep/find/ls/request_scope
  psi-safety/    # Risk tiers, task contract, bash classifier, policy engine, sandbox backends, proxy, guards
  psi-session/   # JSONL tree sessions, compaction, checkpoint index
  psi-ext-api/   # API schema (api/*.toml) + build.rs codegen
  psi-ext/       # mlua host: loading, actors, capability checks, events, UI bridge
  psi-tui/       # Terminal UI: renderer, components, editor (≈ pi-tui)
  psi-cli/       # Binary: modes (interactive/print/json/rpc), commands, config, resources (≈ pi-coding-agent)
docs/               # User + agent docs (embedded into binary)
examples/extensions # Example extensions (embedded; also used as tests)
extensions-bundled/ # Built-in features written in Lua (dogfooding)
rules/              # Built-in bash rule tables (embedded)
```

### 3.2 Data flow

```
 user input ──► psi-cli ──► psi-agent loop ──► psi-ai (provider stream)
                  │               │   ▲                 │
                  │               │   └── tool results ◄┤
                  │               ▼                     ▼
                  │          psi-safety  ◄── tool call (parsed, repaired)
                  │  (classify → policy → approve? → checkpoint → sandbox + proxy)
                  │               │
                  │               ▼
                  │          psi-tools ──► filesystem / sandboxed bash
                  │
                  ├──► psi-ext (Lua actors) ◄── events from agent/session/tools
                  ├──► psi-session (JSONL append, tree, compaction)
                  └──► psi-tui (render, dialogs, approvals)
```

### 3.3 Concurrency model

- One tokio multi-threaded runtime for I/O: providers, tools, file operations and the network proxy.
- The TUI runs its render and input loop on a dedicated task or thread, and talks to the agent over channels.
- **Each extension gets its own Lua state on its own dedicated thread, as an actor** (D-13).
  - Messages in: events and tool invocations. Messages out: results and UI requests.
  - Async host functions (such as `ui.confirm` or `model.complete`) use mlua's async support on a per-actor single-threaded runtime. From Lua they look blocking.
  - Render calls to extension UI components are synchronous requests with a deadline (for example 8ms). On timeout, the last frame is reused and the extension is flagged.
- Per-call instruction limits (via mlua hooks) and per-state memory limits apply to every Lua state, trusted ones included, at more generous values. Verify both work on Lua 5.4 with mlua in Phase 5's first week.

---

## 4. Providers and models (`psi-ai`)

### 4.1 Provider support

- **OpenAI-compatible Chat Completions with streaming.** This covers llama.cpp server, Ollama's `/v1` endpoint, LM Studio and vLLM. It's the priority.
- Later, via extension or optional feature: Anthropic Messages and the OpenAI Responses API.
- Custom providers can be registered from Lua (`psi.register_provider`), as in pi.

### 4.2 Model profiles (`models.json`)

Small models differ wildly, so each model gets a **profile** describing its quirks:

```jsonc
{
  "models": [{
    "id": "local-coder",
    "provider": "openai-compat",
    "base_url": "http://127.0.0.1:8080/v1",
    "context_window": 32768,
    "max_output": 4096,
    "tool_mode": "native",            // native | text  (D-09; the eval suite sets this per model)
    "supports_json_schema": true,     // can we constrain tool args?
    "reasoning": { "format": "think_tags", "strip_from_history": true },
    "tokenizer": "endpoint",          // endpoint | estimate  (D-20)
    "temperature": 0.2,
    "max_tools": 8                    // cap active tools for this model
  }]
}
```

### 4.3 Tool-call robustness (key for weak models)

Run this pipeline on every tool call:

1. **Parse.** Take native `tool_calls`, or the text protocol in `text` mode (D-09).
2. **Repair the JSON leniently.** Fix trailing commas, single quotes, unquoted keys, truncated closing braces and code-fenced JSON (**[G-11]**: evaluate existing crates or write our own).
3. **Resolve the tool name.** Match exactly, then by alias table (`read_file`→`read`, `run`/`shell`/`execute`→`bash`, `str_replace`→`edit`), then by edit distance ≤ 2 when unambiguous.
4. **Coerce arguments against the schema.** `"10"`→`10` for integers, a single string into a one-element array, and drop unknown fields with a note.
5. **Validate.** On failure, return a **short** error with one correct example call. Never send a schema dump.
6. **Constrain where possible.** When the backend supports grammars or JSON-schema decoding (llama.cpp grammars, vLLM guided decoding, Ollama `format`), use it for text-mode tool calls.

**Text tool protocol** (D-09), for models with poor native tool calling:

````
<tool name="read">
{"path": "src/main.rs", "offset": 1, "limit": 200}
</tool>
````

There is one tool call per `<tool>` block. Everything outside the blocks is assistant text.

### 4.4 Context accounting (D-20)

- Use `usage` from responses when the backend provides it.
- Otherwise use the backend's tokenize endpoint (llama.cpp has one).
- Otherwise estimate conservatively at **3 characters per token**, since code tokenizes densely.
- Track the context budget per turn. Auto-compaction triggers as described in §7.3.

---

## 5. Agent loop (`psi-agent`)

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

## 6. Core tools (`psi-tools`)

Pi's default four (read, write, edit, bash), plus structured read-only helpers that are **on by default** (D-17): every structured search a small model makes is a bash call that didn't need classifying.

| Tool | Risk tier | Notes for weak models |
|---|---|---|
| `read(path, offset?, limit?)` | R0 | Line-numbered output. Truncation notice says exactly how to continue ("call read with offset=201"). Images later. |
| `write(path, content, intent?)` | R1 in workspace | Checkpoints before writing. Refuses to overwrite a file not read this session unless `force: true` (catches hallucinated overwrites). |
| `edit(path, old_text, new_text, intent?)` | R1 in workspace | See edit matching below. Multiple matches → error listing line numbers. |
| `bash(command, timeout?, intent?)` | Classified (§8.3) | Runs in the sandbox behind the proxy. Output is truncated head+tail with a notice. Non-zero exit status is reported clearly. |
| `grep(pattern, path?, glob?)` | R0 | Ripgrep's library crates (`grep`, `ignore`). Respects .gitignore. Result count is capped. No preprocessor support, so nothing can run commands. |
| `find(glob, path?)` | R0 | Respects .gitignore. Capped. Implements no `-exec`, `-delete` or file-writing actions. |
| `ls(path?)` | R0 | Capped, sorted, marks directories. |
| `request_scope(capability, reason)` | — | Asks the user to expand the task contract (§8.2). Returns granted, denied, or granted with a narrowed scope. |

**Edit matching (D-10):**
1. Exact match → apply.
2. Whitespace-normalized match (trailing whitespace, indentation width, line endings), unique → apply, and say so in the result.
3. Otherwise → do **not** apply. Return the closest candidate region with line numbers and a similarity score so the retry is cheap. A silently wrong edit is worse than one extra turn.

**Intent parameter (D-19):** `intent` (one short sentence) is optional on write, edit and bash. It is **required when the action classifies as R2 or higher**. If it's missing, the call returns: "This action needs an `intent`: one sentence explaining why. Retry with intent." It is shown in approval dialogs and logged.

**Native-tool hints:** if a bash command is only `grep`, `rg`, `find`, `ls` or `cat` with simple arguments, the result includes a one-line hint such as "Tip: use the `grep` tool for structured results."

**User bash** (pi parity): `!cmd` in the editor runs a command as the user. It goes through policy at user-level trust, and its output is optionally added to the context.

---

## 7. Sessions (`psi-session`)

### 7.1 Format (pi-like)

- One JSONL file per session under `~/.psi/sessions/<cwd-slug>/<timestamp>_<id>.jsonl`.
- Every entry has `id`, `parent_id`, `type` and `ts`, which makes the session a **tree**.
- Entry types:
  - Conversation: `message` (user/assistant/system), `tool_result`.
  - Extension data: `custom` (not sent to the model), `custom_message` (sent to the model).
  - Maintenance: `compaction`, `label`.
  - Psi-specific: `checkpoint`, `policy_decision`, `contract_change`.
- Commands: `/new`, `/resume`, `/tree` (navigate and branch), `/fork`, `/label`, `/export` (HTML later).

### 7.2 Checkpoints and undo (D-05, D-06)

A **shadow git repository** whose `GIT_DIR` lives in `~/.psi/checkpoints/<cwd-slug>/`, with the user's workspace as its work tree, driven through the `git` CLI.

- Snapshot at the start of every turn and before every bash call classified R1 or higher. Only whole-workspace snapshots capture bash side effects.
- It respects `.gitignore` and has a size cap, skipping large binaries with a warning. Watch snapshot time on large repos and log it.
- Each snapshot is recorded as a `checkpoint` entry.
- Commands:
  - `/undo` restores files to the checkpoint before the last turn.
  - `/rewind` picks a tree node and restores both the conversation branch and the files.
  - Restores themselves get a checkpoint first, so they can be undone too.
- If `git` is missing, startup warns that checkpoints are disabled, and the default mode drops to `strict`.
- Writes outside the workspace aren't covered by the shadow repo. That's one reason they're R2 or higher.

### 7.3 Compaction (D-16)

- **Trigger:** compact when the remaining room in the context window would fall below `max_output + max_tool_result_size`, or when usage passes **70%**, whichever comes first. A fixed percentage alone is too coarse for 8–16k windows. Also available manually with `/compact [focus]`.
- Small models summarize poorly, so use a **structured template**: goal, constraints, decisions made, files touched with a one-line purpose each, current state, open problems, next step.
- The last N turns are kept verbatim. The task contract and file-change list are always preserved.
- An optional `compaction_model` (a stronger or faster model) can be configured.
- Extensions can override the summary via the `session_before_compact` event.

---

## 8. Safety system: "freedom with intent" (`psi-safety`)

### 8.1 Risk tiers

| Tier | Meaning | Examples | Default (`guarded`) |
|---|---|---|---|
| **R0 Observe** | No side effects | read/grep/find/ls; `git status`/`diff`/`log`; read-only bash pipelines of R0 commands | Allow |
| **R1 Reversible-local** | Side effects confined to the workspace and checkpointed | write/edit in the workspace; sandboxed bash with workspace-write; unknown commands (the sandbox contains them) | Allow + checkpoint |
| **R2 Scope-expanding** | Reversible with effort, or leaves the contract | a new network domain, package installs, writes outside the workspace, long-running/background processes, reading secret-pattern files (D-12), unparseable or dynamic bash | Needs intent; ask unless granted in the contract |
| **R3 Irreversible/external** | Can't be undone locally | `git push`, publishing, deleting outside the workspace, `sudo`, unsandboxed execution | Needs intent; always ask (unless the contract has an explicit standing grant) |
| **Forbidden** | Hard deny | disabling the sandbox from within a session; reading `~/.ssh/id_*`; writing psi's own config or policy files | Deny (only config edited by the user can change these) |

**Modes** (D-11), switchable with `/mode`:

- `strict`: ask for R1 and above.
- `guarded` (default): ask for R2 and above unless granted.
- `free`: R0–R2 automatic within the sandbox and checkpoints; ask for R3.

Every mode keeps the sandbox, proxy, checkpoints and forbidden list.

**Secret-pattern reads (D-12):** files like `.env`, `*.pem` and `credentials*` are R2 in the first release. A later refinement **[G-17]**: allow the read but mark the session as having touched secrets, and require an explicit ask before any new network domain afterward. Reading a secret isn't the danger; sending it somewhere is.

### 8.2 The task contract (the "intent" scope)

Each session has a **contract** that is visible in the status bar and editable with `/contract` and `/grant`:

```jsonc
{
  "workspace": "/home/me/proj",
  "extra_paths": { "read": ["~/notes"], "write": [] },
  "network": { "mode": "proxy", "allow": ["crates.io", "static.crates.io"] },  // off | proxy | full
  "grants": ["pkg-install"],        // standing R2/R3 categories the user pre-approved
  "budgets": { "max_turns": 40, "max_tool_calls": 150, "max_consecutive_errors": 4, "wall_minutes": 30 }
}
```

- Defaults come from global settings, then project `.psi/settings.json`, then CLI flags.
- **`request_scope`** lets the model ask for an expansion. The approval card shows the reason, and the user picks:
  - once,
  - for the session (adds a grant),
  - deny with a message back to the model.
- This turns surprise denials into explicit, explainable requests, which small models handle well.

### 8.3 Bash classification (D-21)

Ported in spirit from v1's mvdan/sh parser: **approve every command in the parsed tree, never the command string.** This is what stops an allowed command from opening a path through pipes.

**Parser:** tree-sitter-bash via the `tree-sitter` crate. Evaluate the `bash-ast` crate (a typed Rust tree over tree-sitter-bash, aimed at permission gating) as a convenience layer; fall back to raw tree-sitter nodes if it doesn't hold up.

**Algorithm:**

1. **Parse.** Any `ERROR` node → R2 ("couldn't parse this command").
2. **Collect every simple command**, recursing into pipelines, `;`/`&&`/`||` lists, subshells, `{ }` groups, `$(…)` and backtick substitutions, `<(…)`/`>(…)` process substitutions, and function bodies.
3. **For each simple command:**
   - The command name must be a literal. `$CMD args` → R2.
   - Unwrap wrappers (`env`, `nice`, `timeout`, `nohup`, `xargs`, `sudo`, `watch`, `time`, `command`, `stdbuf`) and classify the inner command. `sudo` additionally forces R3.
   - `sh -c` / `bash -c` with a **literal** script → parse that script recursively (depth limit 3). With a non-literal argument → R2.
   - `eval`, `source`/`.`, alias definitions, and here-documents fed into an interpreter (`sh`, `bash`, `python`, `node`, …) → R2.
   - Environment assignments are fine, except `PATH=`, `LD_PRELOAD=` and `LD_LIBRARY_PATH=` → R2.
   - Look the command up in the rule table: base tier plus flag-based escalations.
   - Commands not in the table → **R1**. The sandbox and proxy contain them, so they're free to run (principle 1).
4. **Classify redirects by target:** `/dev/null` → R0; a path in the workspace → R1 (write); outside → R2; `/dev/tcp/…` or `/dev/udp/…` → network (R2, and the proxy blocks it anyway).
5. **The overall tier is the highest of all parts.** Collect a reason for each escalation; they appear in approval cards and block messages.
6. Cache classifications by command-string hash.

**Rule tables** (embedded `rules/bash.jsonc`, user overrides in settings; extensions can add rules that only **escalate**):

```jsonc
[
  { "command": "ls",   "tier": "R0" },
  { "command": "grep", "tier": "R0" },
  { "command": "rg",   "tier": "R0",
    "escalate": [{ "flags": ["--pre"], "to": "R2", "reason": "--pre runs a command on every file" }] },
  { "command": "find", "tier": "R0",
    "escalate": [
      { "flags": ["-exec", "-execdir", "-ok", "-okdir"], "to": "inner", "reason": "find runs a command" },
      { "flags": ["-delete"], "to": "R1", "reason": "find deletes files" },
      { "flags": ["-fprint", "-fprint0", "-fprintf", "-fls"], "to": "write-arg", "reason": "find writes a file" }
    ] },
  { "command": "xargs", "wrapper": { "inner": "first_non_flag" } },
  { "command": "git", "tier": "R1",
    "subcommands": { "status": "R0", "diff": "R0", "log": "R0", "show": "R0", "push": "R3" },
    "escalate": [{ "flags": ["-c"], "match": "core\\.pager|alias\\..*=!", "to": "R2", "reason": "git config can run commands" }] },
  { "command": "awk",  "tier": "R0", "escalate": [{ "script_match": "system\\(|\\|\\s*\"", "to": "R1" }] },
  { "command": "sed",  "tier": "R0", "escalate": [{ "flags": ["-i"], "to": "R1" }, { "script_match": "\\be\\b|\\bw ", "to": "R1" }] },
  { "command": "less", "tier": "R2", "reason": "pagers can open a shell; use read instead" }
]
```

Start the table from v1's mvdan/sh rules (§0), then grow it from the eval suite. A fixture suite of 300+ commands, including known escape attempts, gates every change (§14).

### 8.4 Sandbox and network proxy (D-07, D-08)

**bubblewrap on Linux and Seatbelt on macOS, both from Phase 3**, the same pairing Heimdall v1 used.

- **One policy, two backends.** The task contract compiles to a platform-neutral `SandboxPolicy` (readable roots, writable roots, denied paths, network mode). A `Sandbox` trait has two backends:
  - **Linux: bubblewrap.** Read-only root; workspace, temp directory and granted paths bind-mounted writable; denied paths masked; new PID and mount namespaces; `--unshare-net` always.
  - **macOS: Seatbelt.** An SBPL profile generated in memory and passed to `sandbox-exec -p`. Deny by default, broad reads minus denied paths, writes limited to the workspace, temp directory and granted paths. Network limited to the proxy's localhost port.
- **Network goes through a built-in proxy on both platforms.**
  - Psi runs an in-process HTTP CONNECT + SOCKS5 proxy (tokio) that enforces the contract's domain allowlist.
  - Linux: the sandbox has no route to the host network. The proxy's Unix socket is bind-mounted in, and a tiny bridge (the `psi` binary itself, `psi sandbox-bridge`) listens on localhost inside the sandbox and forwards to it. `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` are set in the sandboxed environment.
  - macOS: the Seatbelt profile allows outbound connections only to the proxy's port.
  - The first request to a domain not on the allowlist raises a live approval ("allow `crates.io` for this session?"), recorded as a contract grant. In headless modes it is denied and reported.
  - Known limit: tools that open raw sockets instead of honoring proxy variables fail. The block message says so.
- **Porting from v1:** start from v1's bubblewrap arguments and Seatbelt profiles. Codex's Rust sandbox code (`codex-rs`) and Anthropic's `sandbox-runtime` (Apache-2.0, TypeScript, same bubblewrap/Seatbelt/proxy design) are design references.
- **Startup checks:**
  - Linux: verify `bwrap` is installed and works. Detect Ubuntu 24.04+ AppArmor restrictions on unprivileged user namespaces, which break bubblewrap, and print the exact fix.
  - macOS: verify `sandbox-exec` runs a trivial profile.
  - If no sandbox is available, bash runs only with per-command R3 approval, and the status bar shows `UNSANDBOXED`.
- **Later (Phase 8):** an opt-in `isolated` backend on a libkrun micro-VM (KVM on Linux, Hypervisor.framework on Apple Silicon) for untrusted repositories, behind the same trait (**[G-16]**).
- **Windows:** WSL only (**[G-05]**).
- Unsandboxed execution is always R3.

**Blocked-action messages** must be instructive:

```
BLOCKED (R2: network). example.com is not on this task's allowlist.
If you need it, call request_scope(capability="network:example.com", reason="...").
Otherwise, continue without it.
```

### 8.5 Policy engine

Evaluation order for every tool call:

1. **Forbidden list** (core, user config only).
2. **Classification** → tier, with reasons.
3. **Contract grants** check.
4. **Extension `tool_call` hooks** (pi parity). Hooks can block, ask, modify input or add notes. They **cannot** downgrade a tier or bypass the forbidden list, sandbox or proxy.
5. **Mode rules** decide allow, ask or deny.
6. On allow: checkpoint if R1 or higher, then execute in the sandbox.

Failure handling:
- **Fail closed.** If a hook errors, or a decision can't be made, the call is treated as ask in interactive mode and deny in headless modes.
- Every decision is written as a `policy_decision` session entry: tool, args summary, tier, reasons, intent, decision, decider.

### 8.6 Guardrails for weak models

- **Loop detection.** The same tool name and argument hash appearing 3× in a 10-call window triggers an injected nudge ("You've called X with the same arguments 3 times; the result won't change. Try a different approach or ask the user."). At 5×, pause and ask the user.
- **Error streaks.** After `max_consecutive_errors`, inject a nudge to re-read the relevant files and re-plan. After 2× that, pause.
- **No-progress heuristic.** N turns with no file change, no new file read and no user-facing answer triggers a nudge (**[G-12]**: tune N).
- **Edit assistance** (§6): return candidate regions instead of a bare "not found."
- **Output hygiene.** Truncate tool output with head and tail kept. Strip ANSI codes. Collapse repetitive lines ("… 240 similar lines omitted").
- **Post-edit verification** (bundled extension): optionally run the project's check command (`cargo check`, `npm test`, …) after edits and feed back a short summary.
- **Read-before-write.** Refuse `write` over an existing file the model hasn't read this session, unless `force: true`.

### 8.7 Approval UX

- **Interactive:** a modal card showing the tool, a compact argument summary, the tier, the intent and the classification reasons.
  - Keys: `y` once, `s` session grant, `n` deny, `e` deny with a message.
  - Policy-driven, so extensions can add context lines.
- **Print/JSON modes (D-18):** no prompts. Use flags such as `--allow network:crates.io,pkg-install` and `--mode free`. Anything else above the threshold is denied, and the denial is reported to the model. Unattended runs are exactly where a weak model's mistakes go unnoticed, so they fail closed.
- **RPC mode:** approval requests are forwarded to the client.

### 8.8 Prompt injection (**[G-09]**)

File contents, command output and web pages can contain instructions. Tiers, the sandbox and the proxy bound the damage. Still to design:

- Marking tool output as untrusted data in the prompt.
- A user-visible indicator when an R2+ request follows closely after reading external content.

---

## 9. Extension system (`psi-ext`, `psi-ext-api`)

### 9.1 Runtime (D-02)

**Lua 5.4** via mlua (features: `lua54`, `vendored`, `async`, `serialize`). Weak local models are the bottleneck, so the language they write most reliably wins; Lua 5.4 has more training data than Luau, and the generated stubs plus `ext_check` close most of the type-safety gap. State "Lua 5.4" prominently in every doc, because models mix up 5.1, LuaJIT and 5.4 semantics.

### 9.2 Loading, manifests and trust tiers

**Locations** (pi parity):
- `~/.psi/extensions/` (global).
- `<project>/.psi/extensions/` (project; requires project trust, see below).
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
  setup = function(psi)
    psi.on("tool_call", function(ev, ctx)
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
- **Trusted:** all capabilities, including the `fs`, `process` and `net` host APIs. Raw `io` and `os` stay off by default, so that policy still sees an extension's effects through host APIs (**[G-13]**: confirm this doesn't block legitimate trusted use).

**Approval:**
- New or changed extensions show their manifest for approval: tier plus capabilities.
- Changing capabilities requires re-approval.
- **Project trust** (pi parity): project-local extensions and settings require a one-time per-project trust prompt.

**Extension tools pass through the same policy.**
- A registered tool declares `tier` (R0–R3) and annotations: `read_only`, `destructive`, `open_world`, mirroring the MCP-style hints pi uses.
- A sandboxed extension cannot declare a tier lower than its capabilities imply. For example, a tool from an extension with `net` is at least R2.

### 9.3 API surface (pi parity target)

| Pi | Psi (Lua) | Phase |
|---|---|---|
| `pi.on(event, handler)` | `psi.on(event, fn(ev, ctx))` | 5 |
| `pi.registerTool` | `psi.register_tool{ name, description, params, tier, annotations, execute }` | 5 |
| `pi.registerCommand` | `psi.register_command(name, { description, handler })` | 5 |
| `pi.registerShortcut` / `registerFlag` | `psi.register_shortcut` / `register_flag` | 5 |
| `pi.sendUserMessage` / `sendMessage` | `psi.send_user_message` / `psi.send_message` | 5 |
| `pi.appendEntry` | `psi.append_entry` | 5 |
| `pi.setActiveTools` / `getActiveTools` | `psi.set_active_tools` / `get_active_tools` | 5 |
| `pi.registerProvider` | `psi.register_provider` | 7 |
| `pi.registerToolRenderer` | `psi.register_tool_renderer` | 6 |
| `pi.events` (inter-extension bus) | `psi.events.emit/on` | 6 |
| `ctx.ui.*` | `ctx.ui.notify/confirm/select/input/status/widget/custom` | 5–6 |
| `ctx.executeTool` | `ctx.execute_tool(name, args)` (goes through policy) | 6 |
| `pi.registerMcpServer` | **[G-08]** | later |

**Events** (start with this subset, using pi's names where they apply):
- Input: `input`, `before_agent_start`.
- Runs and turns: `agent_start`, `agent_end`, `turn_start`, `turn_end`, `context`.
- Messages: `message_start`, `message_update`, `message_end`.
- Tools: `tool_call` (gate), `tool_execution_update`, `tool_result` (chain).
- Session: `session_start`, `session_shutdown`, `session_before_compact`.
- Psi-specific: `policy_decision` (notification), `scope_request` (gate).

**Composition semantics** (pi parity): each event declares one kind:
- **notification:** return values are ignored.
- **mutation:** later handlers see in-place changes.
- **chain:** each handler receives the previous handler's return value.
- **gate:** the first block wins and the operation stops.

Handler errors are reported and never crash the host. A failing `tool_call` gate fails closed.

### 9.4 Single-source API definition (D-14)

**Every Lua-visible function is defined once,** in TOML files under `crates/psi-ext-api/api/`. `build.rs` generates everything else, so the API the model reads can never drift from the API that runs.

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
  { name = "tool",    type = "string" },
  { name = "input",   type = "table" },
  { name = "tier",    type = "\"R0\"|\"R1\"|\"R2\"|\"R3\"" },
  { name = "reasons", type = "string[]" },
  { name = "intent",  type = "string?" },
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

1. **`ExtHost` Rust trait.** One method per function with typed arguments. The host implements it, so the compiler flags any function that's defined but not implemented.
2. **mlua registration glue.** Builds the `psi` global table, checks capabilities on each call, and converts arguments with precise errors. Example: `psi.ui.confirm: argument 2 'message' expected string, got nil`.
3. **LuaLS stub** at `docs/api/psi.lua` (`---@meta`, `---@class`, `---@param`, `---@return`, `---@alias` for events).
4. **Markdown reference** at `docs/api.md`, grouped by capability, including event-kind semantics.
5. **Signature table** (JSON) consumed by `ext_check` (§9.5).

A CI check regenerates these artifacts and fails if anything committed differs.

Example of generated stub output:

```lua
---@meta
---@class psi.ui
local ui = {}

---Ask the user a yes/no question. Returns false in headless modes.
---Requires capability: ui
---@param title string
---@param message string
---@return boolean
function ui.confirm(title, message) end
```

Workflow for adding a function: add the TOML entry → `cargo build` fails naming the missing `ExtHost` method → implement it → rebuild, and the glue, stubs, docs and signature table all update.

### 9.5 `ext_check`: static and dry-load validation (D-15)

Available as an **agent tool** (R0) and as a CLI command, `psi ext check <path>`.

1. **Manifest.** The file returns a table with the required fields; capabilities are known names; the tier is valid.
2. **Syntax.** Compile the chunk without executing it, and report line and column.
3. **Static checks**, a custom pass using the `full_moon` parser (always available, schema-aware):
   - Unknown `psi.*` and `ctx.*` members, against the signature table.
   - Argument count against the signature.
   - Literal type mismatches.
   - Accidental globals (assignment without `local`).
   - Common Lua pitfalls: `!=`, `continue`, `#` on sparse tables in obvious cases, `string.match` used with regex syntax.
   - Also run `lua-language-server --check` when it's installed.
4. **Dry load.** Run `setup(psi)` against a **recording host** that has no side effects. Capture registrations (tools, commands, events). Validate tool parameter schemas and event names. Errors include a traceback.
5. **Output** is compact structured diagnostics, which small models handle better than prose:

```json
{"ok": false, "diagnostics": [
  {"file":"guard.lua","line":12,"col":9,"severity":"error",
   "msg":"unknown member psi.ui.confirmm","hint":"did you mean psi.ui.confirm?"}
], "registered": {"tools":["safe_push"],"commands":[],"events":["tool_call"]}}
```

### 9.6 `psi ext test`: headless extension tests

- Test files named `*_test.lua` live next to the extension.
- Runnable via the CLI, or the `ext_test` agent tool (R1: it runs code, but in a fake host).
- Test API (generated stubs cover it too):

```lua
local t = require("psi.test")

t.case("blocks force push to main", function(h)
  h.load("./git-guard.lua")
  local r = h.emit("tool_call", {
    tool = "bash", tier = "R3", reasons = { "git push" },
    input = { command = "git push --force origin main" },
    intent = "publish fix",
  })
  h.expect(r).to_match({ block = true })
end)

t.case("asks before normal push", function(h)
  h.load("./git-guard.lua")
  h.ui.on_confirm(function(title, msg) return true end)  -- scripted user
  local r = h.emit("tool_call", { tool = "bash", tier = "R3", reasons = { "git push" },
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
  - lines are strings that may be styled with `psi.style.*` helpers, which emit ANSI;
  - render runs under a deadline and an instruction cap (§3.3).
- Custom renderers for tool calls and results arrive in phase 6.

### 9.8 Bundled extensions (dogfooding)

Ship these as real Lua extensions. They serve as working features and as the agent's best examples:

- `permission-presets`: pre-defined policy rule sets, such as "git safety" and "no package installs."
- `post-edit-check`: runs a configured check command after edits (§8.6).
- `todo`: a branch-aware todo tool that rebuilds its state from session entries.
- `plan-mode`: a read-only exploration mode that proposes a numbered plan.
- `git-guard`: the example from §9.6.

---

## 10. Self-extension: guiding the LLM

### 10.1 Docs map in the system prompt (pi parity)

Pi's prompt points the model at its installed docs and examples, and tells it to read them completely before working on pi topics. Do the same:

- On first run (and on version change), extract the embedded `docs/` and `examples/` to `~/.psi/docs/<version>/`.
- Add a short section to the system prompt:

```
Psi documentation (read only when the user asks about psi itself, extensions, skills, themes, or the TUI):
- Docs: ~/.psi/docs/<v>/README.md and ~/.psi/docs/<v>/docs/
- API stubs (exact signatures): ~/.psi/docs/<v>/docs/api/psi.lua
- Examples: ~/.psi/docs/<v>/examples/extensions/ and bundled extensions
Extensions are Lua 5.4. When writing one: read docs/extensions.md fully, start from the smallest
matching example, then run ext_check, then write a *_test.lua and run ext_test until it passes.
```

### 10.2 Docs set (write for agents and humans)

- `docs/extensions.md`: lifecycle, a "choose an event by intent" table (pi style), tiers and capabilities, and patterns.
- `docs/lua-pitfalls.md`: Lua 5.4 specifically; 1-based indexing, `~=`, no `continue`, Lua patterns versus regex (use `psi.re`), `#` with nil holes, `:` versus `.`, integer division `//`.
- `docs/safety.md`: tiers, the contract, how to declare tool tiers and how to request scope.
- `docs/tui.md`, `docs/skills.md`, `docs/settings.md`, `docs/api.md` (generated).
- A **`writing-extensions` skill** that loads on demand (progressive disclosure, so the base prompt stays small).

### 10.3 Host helpers (so models don't hand-roll them)

`psi.json`, `psi.re` (Rust regex), `psi.path`, `psi.fs` (capability-gated, policy-checked), `psi.http` (capability `net`, through the proxy), `psi.process` (trusted only, policy-checked), `psi.style`, `psi.log`.

---

## 11. Interactive TUI (`psi-tui`) (D-03)

**A custom line-based differential renderer on crossterm, rendering inline** (no alternate screen), similar to how pi-tui works.
- Long agent sessions benefit from native scrollback and text selection.
- It matches the `render(width) → lines` component model that extensions use.
- It costs more Phase 4 work than ratatui would, but switching renderers later would mean rewriting every component.

Must-haves:
- A multi-line editor with history.
- `@` file references with fuzzy completion.
- `!cmd` user bash.
- Slash-command completion.
- Streaming markdown rendering, with code highlighting later.
- Collapsible tool calls and results.
- A status bar showing model, context usage, mode, contract summary (including network allowlist size), active extensions, checkpoint count, and `UNSANDBOXED` when applicable.
- The approval card (§8.7).
- `/tree` navigator.
- Steering and follow-up input while the agent runs.
- Keybindings in `keybindings.json`; themes in `themes/*.json` (pi parity).

---

## 12. Configuration and resources (D-04)

**JSON with comments (JSONC)** everywhere: pi parity, reliable LLM editing, and comments for explaining policy ("why is this allowed?"). Extension manifests are the exception; they live inside the Lua files.

```
~/.psi/
  settings.json        # defaults: model, mode, contract defaults, budgets, bash rule overrides, UI prefs
  models.json          # model profiles (§4.2)
  keybindings.json
  AGENTS.md            # global instructions
  extensions/  skills/  prompts/  themes/  packages/
  sessions/  checkpoints/  docs/<version>/  logs/
<project>/
  AGENTS.md            # project context (also read CLAUDE.md for compatibility)
  .psi/settings.json  extensions/  skills/  prompts/
```

- **Skills** (pi parity): `SKILL.md` with name and description frontmatter. Only the descriptions go in the prompt; the model reads the full file when it's relevant.
- **Prompt templates:** `prompts/name.md` → `/name args`.
- **Packages** (phase 7, **[G-04]**): `psi install git:<url>` or a local path. A package manifest lists extensions, skills, prompts and themes. Installation shows the aggregate capability manifest for approval.

---

## 13. Roadmap

Each phase ends with its acceptance criteria passing in CI, where automatable.

**Phase 0: skeleton.**
- Workspace and crates, CI on Linux and macOS (fmt, clippy, tests), the `psi --version` binary, tracing and logging, JSONC config loading.

**Phase 1: providers (`psi-ai`).**
- An OpenAI-compatible streaming client, model profiles, native and text tool modes, the repair pipeline, a mock provider and a *weak-model simulator* mock (emits malformed JSON, wrong tool names, repeated calls).
- ✅ The repair pipeline passes a fixture suite of 50+ malformed tool calls.
- ✅ `psi -p "hi"` streams from a local llama.cpp or Ollama server.

**Phase 2: agent loop, core tools and sessions** (print and JSON modes).
- Turns, queues, abort, budgets; read/write/edit/bash (unsandboxed for now, behind a dev flag)/grep/find/ls; JSONL tree sessions; `--resume`.
- ✅ A mock-provider end-to-end test edits a file correctly.
- ✅ The session file round-trips.
- ✅ Edit applies exact and whitespace-normalized matches, and returns candidate regions on a miss.

**Phase 3: safety core.**
- Tiers, the bash classifier and rule tables (seeded from v1), the contract, the policy engine, the bubblewrap and Seatbelt backends, the network proxy with domain allowlists and live approvals, startup environment checks, shadow-git checkpoints with `/undo`, loop and error guards, instructive block messages, headless approval flags, policy-decision logging.
- ✅ The classifier passes a 300+ command fixture suite, including pipe, substitution, wrapper and `find -exec` escape attempts.
- ✅ A sandbox suite on **both Linux and macOS**: writes outside the workspace fail, denied paths are unreadable, non-allowlisted domains are blocked, allowlisted domains work through the proxy, escape attempts fail.
- ✅ Every simulator failure mode triggers the correct guard.
- ✅ `/undo` restores files after both edits and bash changes.

**Phase 4: interactive TUI.**
- Renderer, editor, streaming output, tool display, approval card, status bar, slash commands, steering and follow-up, `/tree`, `/mode`, `/grant`, `/contract`.
- ✅ A manual QA checklist.
- ✅ Render benchmarks: no visible flicker; 10k-line sessions scroll smoothly.

**Phase 5: extension runtime.**
- Verify mlua memory limits and instruction hooks on Lua 5.4 first. Then the API schema and codegen (§9.4), Lua actors, manifests, tiers, capabilities, the event subset, tools and commands, UI primitives, `/reload`, project trust.
- ✅ Generated stubs, glue and docs are drift-free in CI.
- ✅ A sandboxed extension cannot exceed its capabilities.
- ✅ A runaway loop is killed without freezing the UI.

**Phase 6: self-extension tooling.**
- Embedded docs and extraction, the docs map in the prompt, `ext_check`, `ext test`, the `writing-extensions` skill, bundled extensions, custom UI components, tool renderers, the inter-extension event bus.
- ✅ Given "write an extension that asks before git push," a reference local model (**[G-01]**) produces an extension that passes `ext_check` and its own tests within N attempts.

**Phase 7: resources and packages.**
- Skills, prompt templates, themes, keybindings, packages, `register_provider`.

**Phase 8: polish and portability.**
- Optional libkrun `isolated` sandbox backend (**[G-16]**), RPC mode, HTML export, compaction tuning, the secret-taint refinement (**[G-17]**), the weak-model evaluation suite in CI (nightly), MCP (**[G-08]**).

---

## 14. Testing strategy

- **Unit and fixture tests:**
  - The repair pipeline.
  - Bash classification: a large table of commands → tier and reasons, including escape attempts.
  - The edit matcher.
  - Session tree operations.
- **Weak-model simulator:**
  - A scripted mock provider that reproduces typical small-model failures: malformed JSON, invented tools, edits against stale content, loops, ignoring block messages.
  - It makes guardrail tests deterministic.
- **Sandbox and proxy tests:** an escape-attempt and proxy suite on Linux and macOS CI runners (**[G-14]**: confirm the Linux runner allows unprivileged user namespaces for bubblewrap).
- **Real-model evaluation** (**[G-01]**, **[G-02]**):
  - A small task suite run nightly against 2–3 reference local models.
  - Metrics: task success, policy violations attempted, blocks recovered from, loops triggered, average turns.
  - Use it to tune guards, prompts, rule tables and each model's `tool_mode`.
- **Extension tests:** every bundled extension and example has `*_test.lua`, run in CI with `psi ext test`.

---

## 15. Decision log

| ID | Decision | Why |
|---|---|---|
| D-01 | Name: **Psi** (ψ); binary `psi`; successor to Heimdall v1 | A Greek-letter nod to pi |
| D-02 | **Lua 5.4** via mlua | Weak models write the language with the most training data most reliably; stubs and `ext_check` close most of Luau's type advantage |
| D-03 | **Custom inline differential renderer** on crossterm | Native scrollback and selection; matches the line-based extension component model; switching later would mean rewriting every component |
| D-04 | **JSONC** config | Pi parity; LLMs edit JSON reliably; comments explain policy |
| D-05 | **Shadow git repo** for checkpoints | Only whole-workspace snapshots capture bash side effects |
| D-06 | **`git` CLI** for v1 | Battle-tested and nearly always installed; move to `gix` only if it hurts |
| D-07 | **Linux and macOS sandboxes from Phase 3**; Windows via WSL | Both platforms matter; the policy is shared, so the second backend is cheap |
| D-08 | **bubblewrap + Seatbelt**, with a built-in filtering proxy | Proven in Heimdall v1; the proxy gives per-domain control on both platforms, which Seatbelt can't do natively |
| D-09 | Tool-call mode **per model profile**, native by default, text-protocol fallback | Small-model chat templates often break native tool calls; the eval suite picks the mode per model |
| D-10 | Exact and whitespace-normalized edits apply; **fuzzy never auto-applies** | A silently wrong edit is worse than one retry; candidates make the retry cheap |
| D-11 | Default mode **`guarded`** | Checkpointed workspace changes run freely; anything leaving the contract asks — "freedom with intent" |
| D-12 | Secret-pattern reads are **R2** | Simple and safe for now; taint tracking is G-17 |
| D-13 | **One Lua state per extension, actor on a dedicated thread** | mlua states aren't `Send` by default; isolates hangs; per-extension memory accounting |
| D-14 | **TOML schema + `build.rs` codegen** for the extension API | One source generates the Rust trait, glue, stubs, docs and `ext_check` table; a proc-macro can't easily produce the non-Rust outputs |
| D-15 | `ext_check` = **custom `full_moon` pass + optional LuaLS** | Always available and schema-aware; LuaLS is a bonus when installed |
| D-16 | Compaction when remaining room < `max_output + max_tool_result_size`, **or 70%** | A percentage alone is too coarse for 8–16k windows |
| D-17 | **grep/find/ls on by default** | Every structured search is a bash call that didn't need classifying |
| D-18 | Headless approvals **fail closed**, with `--allow` flags | Unattended runs are where a weak model's mistakes go unnoticed |
| D-19 | `intent` **optional, required for R2+** | Requiring it everywhere wastes tokens and lowers tool-call success; at R2+ the reasoning and the audit trail earn their cost |
| D-20 | Tokens: **response usage → tokenize endpoint → 3 chars/token estimate** | Exact when possible; conservative otherwise because code tokenizes densely |
| D-21 | **tree-sitter-bash**, every simple command checked, data-driven rule tables | Ports v1's mvdan/sh approach; closes the pipe and substitution hole; the sandbox backstops classifier bugs |

---

## 16. Gap index

| ID | Topic | Status |
|---|---|---|
| G-01 | Reference local models and backends | **Choose 2–3 models across sizes** (use v1's experience, §0) |
| G-02 | Evaluation task suite | Design 20–30 representative tasks |
| G-04 | Package format and registry | Design in Phase 7 |
| G-05 | Windows support | WSL only for now |
| G-07 | Sub-agents | Extension-level; design host hooks |
| G-08 | MCP support | Pi has it built in; Phase 8 |
| G-09 | Prompt-injection mitigations | Design beyond tiers |
| G-11 | JSON repair crate | Evaluate or write our own (Phase 1) |
| G-12 | No-progress heuristic tuning | Calibrate using the eval suite |
| G-13 | Trusted-tier raw `io`/`os` | Off by default; confirm it doesn't block legitimate use |
| G-14 | CI support for bubblewrap | Verify the Linux runner allows unprivileged user namespaces |
| G-15 | Lessons from Heimdall v1 | Gather from the v1 project (§0) |
| G-16 | libkrun isolated backend | Phase 8: guest image, packaging, macOS codesigning |
| G-17 | Secret-taint tracking | Phase 8: allow secret reads, gate later network access |

(G-03, network domain allowlists, was resolved by the proxy in D-08.)

## Appendix A: candidate crates (verify current versions and maintenance)

`tokio`, `reqwest` (rustls), `serde`/`serde_json`, a JSONC parser, `mlua` (`lua54`, `vendored`, `async`, `serialize`), `crossterm`, `tree-sitter` + `tree-sitter-bash` (+ evaluate `bash-ast`), a SOCKS5/HTTP-CONNECT proxy implementation (evaluate crates or write a small one on tokio), `grep` + `ignore` (ripgrep), `similar` (diffs), `full_moon` (Lua parsing), `toml` (schema codegen), `clap`, `thiserror`/`anyhow`, `tracing`, `unicode-width`, `regex`, `syntect` (later), libkrun bindings (Phase 8).

## Appendix B: core system prompt draft (keep under ~600 tokens)

```
You are a coding agent running inside psi in the user's terminal. You can read files,
write and edit files, and run shell commands to complete the user's task.

Tools: {tool_list}

How to work:
- Read before you edit. Make small, targeted edits. After changes, verify (run tests or the build).
- Your changes inside the workspace are checkpointed and can be undone, so act confidently there.
- Some actions need permission (new network domains, installs, files outside the workspace, git push, etc.).
  For those, include a one-sentence `intent`. If something is BLOCKED, read the reason:
  use request_scope to ask for access, or continue another way. Do not retry the same blocked action.
- Prefer the grep, find and ls tools over shell equivalents.
- If you are stuck or unsure what the user wants, ask a short question instead of guessing.
- Be concise in your replies.

Task contract: workspace={workspace}, network={network_summary}, grants={grants}

{docs_map_section}
{skills_descriptions}
{project_context}
Current working directory: {cwd}
```
