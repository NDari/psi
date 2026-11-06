# Code review, 2026-11-05

A critical review of the whole workspace at `d6419be`, done by seven reviewers in parallel (one per area) and merged here. Nothing was changed. Every area's tests and clippy passed when it was reviewed.

How sure each finding is:
- **checked:** confirmed again by reading the code while merging.
- **reproduced:** the reviewer ran it (psi, a probe crate linking the real crates, or the real tools).
- **read:** found by reading the code, not run.
- **plausible:** depends on something not verified, such as llama-server's behavior.

Known issues already in `known-issues.md` are left out.

## Themes

1. **What the safety checks look at is not what runs.**
   - Trust hashes `.psi/` with a walker that obeys `.gitignore` and skips symlinks, while the loaders do neither (S2).
   - Read-only tool batches skip the policy (S3).
   - `read_only::check` assumes options come first and every word is literal (S4).
   - Always-ask skips a double-quoted `sh -c` script (S5).
   - Each check uses its own model of the input. Making each one use the same discovery or parse as the code it guards would close most of these.
2. **Two copies of one fact drift apart.**
   - The extension spec is parsed by hand in `host.rs` and again in `check.rs` (D7).
   - `{ cancel = true }` is documented, but the host reads `block` (C4).
   - Three places compute "what is sent" for the context (C1).
   - The UI keeps its own copy of the agent's queues (C6).
3. **Big state machines kept in order by hand.**
   - `run.rs`/`run_calls` (about 2000 lines) mixes planning, policy, batching, guards and recording.
   - `ui.rs`'s `App` has about 45 fields and six dialog slots, with their priority repeated in four places.
   - Many of the correctness findings below come from these two.
4. **Errors that disappear.**
   - Chain-handler errors (C3).
   - Gate results that can't convert to JSON (S8).
   - MCP arguments dropped with only a hidden note (C9).
   - The MCP stderr reader dies on bad UTF-8 (C10).
   - A cut line breaks the whole wire log (C12).

## Security and safety

**S1. High, reproduced. Fixed 2026-11-05.** `psi update` runs an untrusted project's package spec through git and executes it.
- **Where:** `psi-cli/src/packages.rs:342-356` (`update` reads both scopes with no trust check) and `:223` (`git clone --quiet <repo> <dir>` with no `--`).
- **Trigger:** a repo commits `.psi/settings.jsonc` with `"packages": ["git:-utouch /tmp/PWNED; git-upload-pack #@x.y:a/b"]`. Running `psi update` inside it makes git take `-u...` as `--upload-pack` and run it through the shell. The reviewer confirmed the marker file was created.
- **Then:** the clone fails and `fetch` runs `remove_dir_all` on the target.
- **Fix:**
  - skip project-scope packages unless the project is trusted;
  - put `--` before the repo;
  - reject a repo or ref that starts with `-`.

**S2. High, checked. Fixed 2026-11-05.** The trust fingerprint skips files the loaders still load.
- **Where:** `psi-cli/src/trust.rs:32-42`: `ignore::WalkBuilder` keeps its default filters (`.ignore`, `.gitignore`, global excludes) and doesn't follow links. The extension, skill and prompt loaders use `read_dir` and follow symlinks.
- **Trigger 1:** `.psi/.ignore` containing `*` makes the snapshot empty. "Nothing to trust" follows, and `.psi/extensions/evil.lua` loads with no question.
- **Trigger 2:** a symlinked extension directory, or a `*.lua` gitignore entry, means later changes never ask again.
- **Fix:** `.standard_filters(false).follow_links(true)`, or hash what the loaders' own discovery returns. Test with a `.ignore` and a symlink.

**S3. High, checked. Fixed 2026-11-05.** Read-only tool calls never reach the approval policy.
- **Where:** `psi-agent/src/run.rs:1491-1523`. A batch of `read_only()` calls goes straight to `invoke`; `decide` is only called from `run_one` (`run.rs:1682`).
- **Effect:** the policy can still say "ask" for read-only tools:
  - an MCP server with `"approval": "ask"` (`ext.rs:169-174`);
  - an extension tool with `always_ask`, or an unisolated script tool.
- **Trigger:** an MCP tool with `readOnlyHint` on a server set to `approval: "ask"` runs without a card. This contradicts §10.2.
- **Fix:** batch only calls whose decision is `Allow`.

**S4. High, reproduced. Fixed 2026-11-05 (option A; what's left is in known-issues.md).** Plan mode's read-only check is easy to get past.
- **Where:** `psi-safety/src/read_only.rs`. This check gates plan mode and read-only sub-agents (scout).
- **Options after operands** (GNU tools allow them, and `options()` stops at the first operand). Each of these returned Ok and wrote a file:
  - `sed 's/a/b/' -i f`
  - `sed -n 1p f --in-place`
  - `sort in -o out`
  - `awk '{print}' f -i inplace`
- **Listed tools that run commands:**
  - `rg --pre rm x .` deleted every file it searched;
  - `fd -x rm` and `fd -X rm`;
  - `awk -i inplace` (only the long `--include` form is checked);
  - `bat --pager "touch pwned"`.
- **Non-literal words are dropped** (`filter_map(lit)`):
  - `x="commit -qm wip"; git $x` made a commit;
  - `git branch "$name"` and `git tag "$v"` create refs.
- **Fix:**
  - look for the dangerous flags in every word, not just the leading options;
  - refuse a git subcommand or operand that isn't literal;
  - block `rg --pre`, `fd -x/-X`, `awk -i/-l` and `bat --pager`.

**S5. High, checked. Fixed 2026-11-05.** Always-ask misses `git push` inside a double-quoted `sh -c` or `eval` that holds an expansion.
- **Where:** `psi-safety/src/always_ask.rs:510-523`: the script is read only when the whole word is literal.
- **Trigger:** each of these is `Clear`:
  - `sh -c "cd $dir && git push"`
  - `bash -c "git push origin $branch"`
  - `sh -c "git push origin HEAD:$(git branch --show-current)"`
  - `eval "git push $remote"`
- **Why it isn't the accepted case:** the push is literal text, so the plan's "dynamic forms" exemption shouldn't cover it.
- **Fix:** parse the string node's inner source as a script, leaving its expansions as expansions.

**S6. Medium, reproduced. Fixed 2026-11-05, with `time -f/-o`, `eval --` and `builtin`; the other smaller gaps are open.** Always-ask loses git's subcommand after a joined non-literal option.
- **Where:** `always_ask.rs:539`.
- **Trigger:** `git --work-tree=$PWD push`, `git --git-dir="$g" push` and `git -C"$d" push` are `Clear`. The separated form `git -C "$d" push` still asks.
- **Fix:** treat a non-literal word whose raw text starts with `-` as an option.
- **Smaller gaps (reproduced):**
  - `git -c alias.p=push p`
  - `/usr/bin/time -f %e git push`
  - `eval -- git push`
  - `$'git' push`
  - `coproc git push`
  - `builtin command git push`
  - `rm -rf "$PWD"` (removes `.git` with the project)
  - `git switch -f` (discards work like `checkout`)

**S7. Medium, checked. Fixed 2026-11-05. An extension's `send_user_message` while idle runs `!` commands and built-in slash commands.**
- **Where:** `psi-cli/src/interactive/ui.rs:1824` calls `submit_from`, which handles `/` at `:1217` and `!` at `:1228`.
- **Trigger:** an extension passes model text through. `!curl ... | sh` runs as the user, outside the sandbox; `/mode auto` changes the safety mode.
- **Plan:** this contradicts D-45 (extensions aren't a side door around the policy).
- **Fix:** for the source "extension", send it as a plain prompt.

**S8. Medium, reproduced. Fixed 2026-11-05. A gate result that can't convert to JSON fails open.**
- **Where:** `psi-ext/src/actor.rs:352` (`from_value(v).unwrap_or(Json::Null)`).
- **Trigger:** a `tool_call` handler returning `{ block = true, reason = "no", why = function() end }` lets the call through, and `psi ext check` reports no warnings.
- **Fix:** treat a conversion error as a handler error, so the gate blocks and the error is reported.

**S9. Medium, read. Fixed 2026-11-05. `psi inspect` prints session text without escaping control characters.**
- **Where:** `psi-lab/src/inspect.rs:576`: `cut()` only replaces `\n`, and with `--step` it returns the text unchanged.
- **Trigger:** a `read` result or the model's text containing ESC, OSC 52 or a title escape reaches the terminal.
- **What's safe:** the TUI's `/inspect` sanitizes.
- **Fix:** show control characters escaped (`\x1b`).

**S10. Low, read. Sandbox edge cases.**
- **Parents of home are allowed as the workspace.** `isolation.rs:96-104` refuses only `/` and home itself, so starting in `/home` makes `~/.psi` writable.
- **Hooks outlive the session.** Bash can write `.git/hooks` and `.git/config`, and the owner's next `git commit` on the host runs a planted hook outside the sandbox.
- **Fix:** refuse any ancestor of home; consider a read-only bind for `.git/hooks` and `.git/config`.

**S11. Low, read. Package path and trust gaps.**
- **`..` as a host:** `git:../skills/foo` resolves into `~/.psi/skills/foo`, and the failed clone then deletes that directory (`packages.rs:110,224`).
- **Packages outside the fingerprint:** project packages under `.psi/git/`, and local package paths, aren't hashed. A `git pull` that changes them loads with no new question.
- **`network` fails open:** `"network": false` or `"Off"` leaves the network on (`safety.rs:117`).
- **A wrong-typed rule is dropped:** a string `always_ask` is silently ignored.

## Correctness

**C1. High, checked. Fixed 2026-11-06. Dropping turns on a context overflow ignores the summary chain.**
- **Where:** `psi-agent/src/messages.rs:216` (`plan_drop`), called at `run.rs:1980`. It walks the whole branch and sizes it including entries already summarized.
- **Effect:**
  - its `first_kept_id` can land before the chain's `keep_from`;
  - the last compaction wins (`compact.rs:251`), so the summaries and the entries they replaced are both sent;
  - the retries fail, and the session stays inflated.
- **Fix:** run it over `branch[keep_from..]`, sized from what is sent.

**C2. Medium, read. After "go on" at the tool-call limit, every later call is refused.**
- **Where:** `run.rs:1132-1136` computes the allowance from `cfg.budgets.max_tool_calls`, not the raised `call_limit` (`run.rs:985`). The refusal text at `:1447` uses the old limit too.
- **Effect:** after 200 calls and a "go on", calls 201 to 400 all get "Not run" and feed the error streak.
- **Tests:** none cover going on past the call limit.

**C3. High, checked. Fixed 2026-11-05. Errors in chain handlers are never shown.**
- **Where:** `psi-ext/src/actor.rs:362-366`, `lib.rs:327`. Only Notify handlers call `report`; a chain event's error is sent back and dropped (`Ok(Ok(Err(_))) => {}`).
- **Trigger:** `psi.on("tool_result", function() error("x") end)` fails silently. This is the hidden-hook failure §9.4 and §9.6 aim to prevent.

**C4. High, checked. Fixed 2026-11-05. `{ cancel = true }`, as documented, doesn't stop compaction.**
- **Where:**
  - the definition, the docs and the LuaLS file say `cancel` (`psi-ext-api/api/psi.toml:253,481`);
  - the host only looks at `block` (`psi-ext/src/lib.rs:319,372`).
- **Tests:** the scenario test passes because it returns `block`.

**C5. Medium, read. `/compact <focus>` drops the focus on the normal path.**
- **Where:** `psi-agent/src/run/fit.rs:332-391`. The focus is added only to the plain fallback, not to the request that extends the last one.
- **Tests:** the only test has no prior request, so it takes the fallback.

**C6. Medium, read. Fixed 2026-11-06 (the left text goes back in the input box, also after /compact). Steering typed during a run that is then stopped is sent later, unseen.**
- **Where:** `ui.rs:1628` clears the UI's `queued` on RunDone. The agent's queues are drained only inside `run_inner` (`run.rs:1083,1090,1150`), after the cancel check.
- **Trigger:** type during `sleep 30`, then press Esc. The next prompt's run sends the old text with nothing on screen.
- **Same with /compact:** a prompt typed while a summary runs is never sent.
- **Fix:** drain both queues when a run ends, and put the text back in the editor.

**C7. Medium, read. Fixed 2026-11-06 (no test: the TUI can't make start_plan fail). A failed ExecutePlan leaves the UI "running" forever.**
- **Where:** `ui.rs:1020-1025` sets `running`, and `worker.rs:140-143` answers `UiMsg::Error`, which doesn't clear it.
- **Effect:** the spinner never stops, and /undo, /new and /plan refuse.
- **Fix:** reply `RunDone` with `Stop::Failed`.

**C8. High, reproduced. Fixed 2026-11-05. Shapefix refuses a new file in a package named like the project.**
- **Where:** `psi-ai/src/shapefix.rs:545-566` (`doubled_cwd`).
- **Trigger:** from `/x/textkit`, a write to `textkit/__init__.py` (while `textkit/` doesn't exist) is refused and told to use `/x/textkit/__init__.py`. The parent is the cwd, which always exists.
- **Where it hits:** the usual Python `pkg/pkg/` layout, and the `multi_split_package` task.
- **Fix:** when the stripped rest has no directory part, refuse only if the fixed path exists.

**C9. High, plausible. MCP schemas lose `$defs`, and arguments outside top-level properties are dropped.**
- **`$defs`:** `psi-ai/src/schema.rs:86-121` rebuilds `parameters` from the top-level properties, so a `$ref` points at `$defs` that are never sent. Pydantic and FastMCP servers do this.
  - **Plausible effect** (llama.cpp's grammar builder, unverified): every request fails while that tool is listed.
- **Dropped arguments:** `shapefix.rs:465` removes every key not in the top-level properties, so tools using `additionalProperties`, a bare object or a top-level `anyOf` get `{}`. The note goes only to a hidden entry.
- **Fix:** send MCP schemas whole, and don't drop unknown keys for MCP tools.

**C10. Medium, checked. One non-UTF-8 stderr line stops the MCP stderr reader.**
- **Where:** `psi-mcp/src/manager.rs:410`: `while let Ok(Some(l)) = lines.next_line()` exits on `InvalidData`.
- **Effect:** the pipe closes, so the server's next stderr write gets EPIPE; Rust servers using `eprintln!` panic.
- **Fix:** read bytes and decode them lossily.

**C11. Medium, plausible. Connecting while llama-server loads uses a 32K fallback window.**
- **Where:** `interactive/mod.rs:195-205`, `run_mode.rs:503`. Only a connection error keeps the poll going; an HTTP 503 ("Loading model") counts as connected with no props.
- **Effect:** a 27B/35B loads long enough to hit this, giving early compaction and smaller caps for the whole session.

**C12. Medium, read. Session and wire-log writes don't recover from a cut line.**
- **Wire log** (`psi-ai/src/wire.rs:41-53,92-107`):
  - one partial line makes the whole log unreadable to replay;
  - `open` then swallows the error and restarts `seq` at 0;
  - it doesn't add a newline after a cut end.
- **Session** (`psi-session/src/store.rs:149-198`): a partial `append` (ENOSPC) leaves bytes the next entry is glued onto, and `branch()` stops quietly at the missing parent, so the context starts mid-conversation.
- **Fix:** truncate back on a failed write, and report a gap in `branch()`.

**C13. Medium, reproduced. Fixed 2026-11-06. Bash output that is one long line comes back empty.**
- **Where:** `psi-tools/src/bash.rs:198`, `text.rs:52-81`: `head_tail` keeps whole lines only.
- **Trigger:** `head -c 40000 /dev/zero | tr '\0' a` returns `[... 1 lines omitted ...]`. Minified JSON and `curl` output come back the same.
- **Fix:** cut long lines first.

**C14. Medium, reproduced. `psi -p ""` runs as interactive mode.**
- **Where:** `run_mode.rs:243-249` derives the mode from an empty prompt.
- **Trigger:** `psi -p "$PROMPT"` with the variable unset runs with no request cap and no server check.

**C15. Low-medium, reproduced. Edit's whitespace-tolerant tier.**
- **Where:** `edit.rs:310-321`.
- **A deletion leaves a blank line:** old `"b\n"`, new `""` on `"a\nb  \nc\n"` gives `"a\n\nc\n"`.
- **CRLF:** new text already in CRLF gets `\r\r\n`.

**C16. Low, read. Smaller ones.**
- **Sub-agent grants:** a sub-agent's "Yes, for this session" grants only its own copy (`sub.rs:98-105`), so later calls ask again.
- **`announced_stop` false positive:** it fires on "Let me know if you want changes." (`guards.rs:130-145`), giving headless runs an extra turn.
- **Results said not to have run:** results held back by `limit_step` say "Not run" although the calls ran (`fit.rs:161-203`).
- **Sub-agent events:** sub-agents fire `session_before_compact`, skipping the `SUB_EVENTS` filter (`fit.rs:308`, `run.rs:1968`).
- **Wall-time budget:** it stops instead of pausing, and nothing outside a test sets it (`run.rs:987`).
- **`os.exit`:** an extension calling it kills psi without restoring the terminal (`psi-ext-api/src/batteries.rs:41`). Reproduced.
- **Busy limit in tests:** `psi ext test` loses it after a nested `test.emit` and hangs on a loop (`testing.rs:397`). Reproduced.
- **Duplicate extension names:** two extensions with the same manifest name both load, and the second one's `ui.custom` goes to the first (`lib.rs:546`).
- **Non-string tool result:** an extension tool returning a non-string sends the model Rust Debug text, `Integer(42)` (`actor.rs:393`). Reproduced.
- **Grader PASS count:** the grader counts every `PASS ` line, repeats included (`psi-lab/src/eval/grade.rs:73`). Project code printing `PASS x` inside the unittest process can raise the count.
- **Session file pick:** a trial takes the first `.jsonl` that `read_dir` gives, and a nested `psi -p` in the trial can make it the wrong one (`trial.rs:276`).
- **Settings overwritten:** `psi install` overwrites `settings.jsonc` with only the packages when it can't read the file as UTF-8 (`packages.rs:231`).
- **Corrupt trust store:** a corrupt `trust.json` reads as empty, and the next trust overwrites every project's entry. Writes aren't atomic.
- **Stale status:** /new, /resume and /fork keep the old session's ctx %, checkpoint count and last answer (`ui.rs:1693`).
- **Dialogs overwrite each other:** they share single slots (`ext_modal`, `picker`, `pending_entries`), so a second one silently replaces the first.
- **No stall timeout:** a server that stops mid-stream blocks `psi -p` forever (`client.rs:62`).
- **Id collisions:** entry ids are 32 random bits with no collision check, and a collision can make `branch()` loop forever (`entry.rs:256`).

## Performance

**P1. High, reproduced. A streaming code block is re-highlighted whole on every frame.**
- **Where:** `ui.rs:2307-2311` calls `render_block` on the whole pending block every 16 ms, then shows its last 6 lines. An open fence stays pending until it closes (`markdown.rs:104-111`).
- **Measured (release):** 500 lines 70 ms per frame, 1000 lines 139 ms. Past about 110 lines it misses §11.3's 16 ms target, so typing lags while the model writes a file in a fence.
- **Fix:** render only the tail, or keep the highlighter's state between frames.

**P2. Medium, reproduced. A failed `edit` on a big file can take seconds to minutes, and abort can't stop it.**
- **Where:** `edit.rs:370-386` runs Levenshtein over every window.
- **Measured:** 5000 lines with a 40-line `old_text` took 17 s in a release build. It runs in `spawn_blocking`.
- **Fix:** filter candidates by line first, and cap the work.

**P3. Low, read. The transcript is rebuilt three to five times per turn.**
- **Where:** `context_size`, `plan_clearing`, `clear_at_cache_break` and `request` each rebuild it.
- **Also:** schemas are re-serialized several times, and `all_schemas()` is called once per tool call (`run.rs:1388`).
- **Fix:** build the request once per turn.

**P4. Low, read. Other small costs.**
- `read` and `grep` load whole files before cutting them.
- `@` completion clones up to 20K strings per keystroke (`complete.rs:108`).
- `!cmd` output has no cap and goes into the session whole (`ui.rs:1351`).
- The checkpoint store is never packed or pruned.
- `Box::leak`ed extension schemas leak again on every sub-agent and `/reload` (`psi-agent/src/ext.rs:55`).

## Design

- **D1. `run.rs` (2000 lines) and `run_calls`.** Plan, decide, execute and annotate are interleaved in one pass, which is how S3, C2 and the held-back "Not run" text slipped in. Splitting it into those phases, and sharing one per-turn view of the request (P3, C1), would remove whole classes of bugs.
- **D2. `ui.rs`'s `App` (2700 lines, about 45 fields, six dialog slots).**
  - The priority of the six slots is kept by hand in `on_key`, `card_choices`, `draw` and `activity`.
  - Run state is split between UI copies and the agent.
  - A dialog queue, and run state that the worker owns and echoes back, would fix C6, C7 and the overwritten dialogs.
- **D3. Trust is computed apart from loading.** See S2 and S11: one discovery function used both to hash and to load.
- **D4. MCP schemas and the flat-params model.** The core tools fit it; real MCP schemas don't (C9).
- **D5. A panic restores the terminal while the UI keeps running.**
  - The panic hook is process-wide (`interactive/mod.rs:327`), so a panic in the worker turns raw mode off under a live UI.
  - The worker's JoinHandle is dropped, so the run never ends on screen.
  - **Fix:** restore only on the main thread, and watch the worker.
- **D6. Two runaway-Lua stops.** `limits.rs` uses per-thread hooks plus overrides; `check.rs` uses `set_global_hook`. One would do.
- **D7. The extension spec is parsed by hand twice.** `host.rs:467-602` and `check.rs:1640-1760` have drifted: the check refuses duplicates, empty descriptions and unknown `required` names, but the host accepts them, and a second `register_tool` replaces the first. One parser that both use would fix it.
- **D8. `check.rs` (2.8K lines)** would split naturally into script, lint, dry-load and LuaLS parts.

## Clarity and dead code

- `annotations.destructive` and `open_world` are documented and linted but ignored (`host.rs:505`).
- An extension tool named like a core tool disappears without a warning (`run.rs:367`).
- `Status.busy` is never set.
- Doc comments sit on the wrong function: "Puts the terminal back" on `on_path` (`interactive/mod.rs:80`), and `sanitize`'s doc on `apply_sgr` (`psi-tui/src/style.rs:200`).
- `psi-session/src/lib.rs:16` still has `TODO(phase-7): /fork`, though fork exists.
- The 1 GiB wire-log cap (§3.4) isn't built and has no TODO.
- The MCP failure text says "gone from the next request"; the list is fixed per run, so it means the next prompt (`manager.rs:302`).
- `skills`/`prompts` setting paths resolve against the cwd, while package paths resolve against the settings file.
- Two test problems in `psi-agent/src/guards.rs`:
  - the unit test at `:298` passes for a different reason than the one it tests;
  - the 200-character tail rule has no test.

## Test gaps

No test covers:
- **Policy and context:** a read-only MCP tool with `approval: "ask"`; going past the call limit; `/compact <focus>` with a prior request.
- **TUI:** an ExecutePlan error.
- **Trust:** a trust snapshot with `.ignore` or a symlink; `psi update` on an untrusted project; git argument injection.
- **Shell checks:** options after operands in `read_only`; a double-quoted `sh -c` in always-ask.
- **Schemas and paths:** an MCP `$ref` schema; the same-named-package path in shapefix.
- **Files and logs:** a cut line in the wire log or mid-session.
- **Output:** control characters in `psi inspect`.

## What held up

- **Checkpoints:** capture and restore held up under every scenario traced.
- **SSE parsing:** it buffers bytes and decodes whole events, so UTF-8 split across chunks is safe.
- **TUI escapes:** the TUI sanitizes in `Span`/`Line` and again on write, and filters OSC 8 URLs. No injection path through the TUI was found.
- **Unicode:** wrapping and the editor cursor are grapheme-aware.
- **Extension actors:** one thread each, with timeouts, and gates that fail closed (except S8).
- **Tests:** most are real scenario tests tied to transcripts, and model-facing text is lean.
