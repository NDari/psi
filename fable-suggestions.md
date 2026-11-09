# Design suggestions, 2026-11-09

A review of Psi's design at `59d8049`, asked as three questions: what to change in the architecture, in the code, and in usability for someone working with a local model such as the Qwen3.8 27B.

**What was read:** `psi-plan.md`, `psi-agent`, `psi-ai`, the tool schemas in `psi-tools`, the settings, `known-issues.md`, `experiments.md` and the 2026-11-05 code review. **Not read:** `psi-ext`, `psi-tui`, `psi-mcp`, `psi-lab` and the internals of `psi-safety`, so nothing here covers them.

**How sure:** every file and line named was read on 2026-11-09. Claims about llama-server or a model's chat template are from memory and marked "to verify".

The design is sound. Most of what follows is about two things: putting model-specific behavior in one place, and making the harness act where it only nudges today.

## Architecture

### A1. The model profile holds everything model-specific, and sampling is sent per request

- **Today:** sampling and template settings are in `justfile:17-36`, thinking is in `~/.psi/models.jsonc` (`crates/psi-ai/src/profile.rs`), guard thresholds are constants, and `ChatRequest::body` sends no sampling at all (`crates/psi-ai/src/types.rs:67`).
- **Why it matters:** a server not started by `just llm-up` runs on the GGUF's defaults. That is how the 35B ran at temp 1.0 until 2026-11-05 (`experiments.md`, "The model card's sampling for the 35B").
- **Change:** `sampling` in the profile (temperature, top-p, top-k, min-p, presence penalty), sent with each request. Sampling fields are not part of the prompt, so the prompt cache holds. A summary request could then use its own sampling.
- **To verify:** that llama-server's chat endpoint takes these fields per request.

### A2. Loop handling ends with the harness acting, on one ladder for the TUI and headless runs

- **Today:** the guards see only a call's signature and whether it failed (`crates/psi-agent/src/guards.rs`), `no_progress` only nudges, and the TUI has no request cap. The 2026-11-06 TUI rerun made 623 calls of two commands over 106 minutes.
- **Change, part 1 (in `known-issues.md` already):** a nudge that goes unheeded becomes a pause.
- **Change, part 2 (a hypothesis):** when a loop is found, cut it out of the context: set the leaf back to before the repeats and append one harness note saying what was run and what it returned. The repeated turns are what prime the next repeat, and each nudge adds to the pattern. Try it with `psi replay` on recorded loop steps before building it.

### A3. The start of the request stays the same bytes by design

- **Today:** plan mode takes write and edit out of the tool list (`crates/psi-agent/src/run.rs:428`), and `before_agent_start` text is added to the system prompt (`run.rs:1198`). Each change makes the server read the whole context again.
- **Why it matters:** the 27B reads 160 to 200 tokens a second on the Mac, so a 40K context is 3 to 4 minutes.
- **Option:** keep the tool list fixed for the session and refuse in the harness (the refusal text exists, `run.rs:1433`); send run-scoped text as a context message after the prompt.
- **Cost:** one wasted turn when the model tries write in plan mode.

### A4. Decide what happens to thinking

- **Today:** thinking is recorded and never sent back (`crates/psi-agent/src/messages.rs:101`), while `justfile:24` starts the 35B with `preserve_thinking: true`. The template never receives the reasoning, so the flag likely does nothing. D-38 lists this as open.
- **Change:** per profile, either send the reasoning back for the steps since the last user message, or drop the flag.
- **To verify:** each model's template with `/apply-template`.

### A5. `run.rs` around one per-turn value

- Code review D1 is still open: `run.rs` is 2016 lines and `run_calls` plans, gates, batches, runs, guards, annotates and records in one pass.
- **Change:** build the request, schemas, offered names and branch once per turn and pass that through the phases. It removes the rebuilds at `run/fit.rs:120`, `:213`, `:243` and `run.rs:1204`, and the `all_schemas()` call per tool call (`run.rs:1387`).
- `if self.sub.is_some()` appears about eight times; a small role struct would replace those checks.

## Code

- **C1. No idle timeout on the stream.** `crates/psi-ai/src/client.rs:61` sets only a connect timeout, and `max_wall` is checked between turns (`run.rs:987`). A server that stalls mid-answer hangs a print run until something outside kills it. Fix: a generous timeout between chunks, with progress events counting as activity.
- **C2. `branch()` is quadratic.** `crates/psi-session/src/store.rs:186-198` scans from the end of the file for every parent, and it is called several times a turn. An id-to-index map fixes it. It matters on sessions with thousands of entries.
- **C3. `psi-agent` returns `Result<_, String>` throughout**, where AGENTS.md asks for `thiserror` in library crates.
- **C4. The context estimate counts thinking that is never sent back.** `estimate_tokens` (`crates/psi-agent/src/compact.rs:95`) adds the last response's output tokens, thinking included. With heavy thinking a compaction can come a few thousand tokens early.

## Usability with a 27B-class model

### U1. Show what the model is doing while it is silent

- **Today:** the agent drops reasoning and tool-call deltas (`run.rs:1264`), so the spinner says "waiting for the model…" (`crates/psi-cli/src/interactive/view.rs:368`) while the model thinks and while it writes a long `write` call. At about 33 tokens a second that is minutes with nothing on screen.
- **Change:** a live line that says "thinking" or names the call being written, with a size that grows. Nothing is committed to the scrollback.
- **Status:** built 2026-11-09 (see "Built" at the end of this file).

### U2. Pause on loops in the TUI

- A2, part 1. The data makes this the top daily-use problem: the first cog prompt looped in 8 of 10 headless runs (`experiments.md`).
- **Status:** built 2026-11-09 (see "Built" at the end of this file).

### U3. `/model`, a thinking toggle, and the profile picked from `/props`

- The first profile is the default whatever the server has loaded (`docs/help/models.md`), so a swapped model silently gets another model's settings.

### U4. Say what a cache break will cost before it happens

- Psi knows the reading speed, so `/plan`, `/reload`, `/compact` and a resume could say "reads about 40K tokens again, about 3.5 minutes".

### U5. Save and restore the server's slot

- For `/resume` and sub-agents, so a session resumed the next day doesn't start with a cold read. `psi-cog-test-issues.md` already names it for sub-agents.
- **To verify:** llama-server's slot save and restore API.

## Order

U1 and U2 first, then A1.

## Built 2026-11-09

### U1: the spinner says what the model is writing

- `psi-ai` streams the pieces of a tool call's arguments (`Delta::ToolCallArgs`).
- `psi-agent` turns thinking and call arguments into sizes (`crates/psi-agent/src/generating.rs`) and sends them as `thinking` and `call_progress` events, at the start and about every 64 characters. A retry counts again from nothing.
- The TUI's live line reads `thinking: 1.2K characters (12s)` or `writing a write call for src/big.rs: 2.8K characters (41s)`, also under answer text that has stopped. No thinking text and no arguments reach the screen.
- Not done: a sub-agent's thinking, and thinking during a summary (each already shows a timer).
- Tests: the stream, the counter, the agent's events through the replay server, and two pty tests with a stream that stalls mid-thought and mid-call.

### U2: an unheeded no-progress nudge becomes a pause

- 12 turns in a row that only repeat earlier calls and change no file pause the run; a headless run stops with `stop: guard`. The nudge still comes at 6, once. Going on starts the count over.
- A summary forgets the earlier calls, so reading a file again after its result was summarized away is not a repeat.
- The rule was picked by replaying it over the recorded sessions and eval trials before writing it (about 800 Psi runs). 18 reach the pause: 7 of the 8 loops of 2026-11-06 in `experiments.md`, the clearing-on review of 2026-11-05, a scout from the cog trial, six of the 2026-11-08 sessions and two failed eval trials. None of the 18 changed a file after that point.
- What it does not catch is listed in `known-issues.md`: a marker that changes with each repeat, a lookup whose flags change, and a cycle that changes files and changes them back.
- The wording the model sees is unchanged. No model run has been made with either change.
