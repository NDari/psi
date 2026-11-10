# Living inside Psi: a survey of quality-of-life options

> **History.** A survey from 2026-11-02, when parity with pi and Lua extensions were goals; `design-going-forward.md` dropped both. Built from its shortlist (`psi help commands`, `psi help settings`): Ctrl+G, notifications and tab progress, Ctrl+R and the draft stash, paste collapse, `/copy`, clickable links, shell completion, the status line. Cut or dropped: the Lua batteries, the docs sub-agent, anything for extensions. The rest are ideas, not plans.

As of 2026-11-02. A survey of what other agents and tools do to make daily use better, measured against what Psi has today. Items are options to discuss, not decisions; the shortlist at the end is a recommendation.

## Contents

1. How to read this
2. Where Psi stands today
3. What other coding agents do
4. Terminal and TUI experience
5. Search and code navigation
6. Lua batteries for extensions
7. Documentation inside the CLI
8. Working with other terminal tools
9. Sessions, history and local-model quality of life
10. Shortlist and open questions

## 1. How to read this

Most of the value is in small, human-side changes that need no eval run: an external editor, notifications, clickable links, word-level diffs, history search. The bigger items (Lua batteries, editor bridges) need a plan edit first. The ranked list is in §10.

- **Effort:** small is under a day, medium 1 to 3 days, large more.
- **Model-facing:** anything that changes the prompt, tool descriptions or tool results needs an eval run (D-32). Those are marked as experiments.
- **Sources:** five research passes on 2026-11-02 opened the linked pages and read pi's source (local clone) and other agents' code on GitHub. "Unverified" marks claims from memory or from pages that couldn't be opened.
- **Fit** is judged against Psi's principles: inline TUI, the harness absorbs model mistakes, lean model-facing text, dependencies welcome when they earn it.

## 2. Where Psi stands today

Psi already covers the core of a daily driver: a fast inline TUI, highlighted markdown, a session tree with undo, Lua extensions and embedded help. The gaps are mostly at the edges, where the terminal and other tools meet Psi. Checked against the code on 2026-11-02.

| Area | What Psi has | What it lacks |
| --- | --- | --- |
| Rendering | Inline differential renderer, synchronized output (mode 2026), syntect highlighting with bat's 213 grammars, pulldown-cmark tables and lists, unified diffs in tool cards, `/show <n>` for a full call | Word-level diffs, side-by-side diffs, clickable links (OSC 8 is stripped, not emitted), images |
| Input | Multi-line editor with history (up and down), `@path` fuzzy completion, slash-command completion, bracketed paste, `!cmd` and `!!cmd`, steering and follow-up queues, kitty keyboard protocol (disambiguate only), `keybindings.json` | History search (Ctrl+R), editing the prompt in `$EDITOR`, vim mode, pasting images, a command palette |
| Feedback | Status line, spinner, `/stats`, BEL when a turn ends or an approval waits while the terminal is unfocused | Desktop notifications (OSC 9/777/99), progress in the tab (OSC 9;4), a live context meter |
| Sessions | JSONL tree, `/tree`, `/rewind`, `/undo`, `/fork`, `/label`, `/resume`, `psi -c`, `/export` (plain), `psi inspect` | Search across sessions, a richer session picker, HTML export |
| Search tools | Optional in-process `grep`, `find`, `ls` (the `ignore` and `regex` crates, off by default, D-17); the model uses `bash` otherwise | rg and fd guaranteed in the sandbox, structural search, a repo map |
| Lua extensions | Lua 5.4 per-extension actors; events, tools, commands, shortcuts, flags, providers, renderers, UI primitives and custom components; standard `io` and `os` | JSON, HTTP, regex, processes with timeouts, sqlite, `require` of shared modules |
| Docs | `psi help topics` and 16 embedded topics, `/help`, `psi ext example`, generated `docs/api.md` and `docs/api/psi.lua` stubs, a prompt line pointing the model at `psi help` | Man pages, shell completions, an in-TUI help overlay, searching the docs |
| Other tools | `!cmd`, packages from git or local paths, MCP servers | A file picker (yazi, fzf), launching lazygit or the editor, IDE bridges |

## 3. What other coding agents do

The big 2026 change: most agents went fullscreen (alternate screen), and many users pushed back because it broke scrollback, copy and tmux. Psi's inline design is now the minority choice, which is a strength if it borrows the fullscreen features that work inline.

**The fullscreen move:**
- Claude Code made fullscreen the default for new users from 2026-05-06; `/tui default` keeps the inline renderer ([docs](https://code.claude.com/docs/en/fullscreen)). Issue #41965, "flicker-free rendering destroys terminal scrollback", drew 17 comments.
- Codex CLI made fullscreen the default on 2026-09-29, with `--no-alt-screen` and `/raw` as escape hatches. Comments were hostile ("broke copy / paste in remote tmux") ([discussion](https://github.com/openai/codex/discussions/49129)).
- pi 1.0 "Changed the default TUI mode to fullscreen"; `tuiMode: "regular"` keeps scrollback ([changelog](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/CHANGELOG.md)). This reverses Mario Zechner's 2025 case for scrollback ([post](https://mariozechner.at/posts/2025-11-30-pi-coding-agent/)). Worth noting, since pi is Psi's behavioral reference.
- Amp went fully fullscreen in Sep 2025 ([news](https://ampcode.com/news)).

**Features worth borrowing that aren't covered in later sections:**

| Feature | Who has it | Fit for Psi |
| --- | --- | --- |
| Collapsible tool output (Ctrl+O) | Claude Code, pi, Gemini, Goose (`/r`) | Inline can only expand the live region or re-print, so "print call n in full" (`/show`, which Psi has) is the inline form. Add a key for "show the last call in full" |
| Paste collapse | Claude Code collapses pastes over 800 characters or 3 lines into `[Pasted text #1 +120 lines]`, strips invisible Unicode (tag and bidi characters), and asks for a second Enter ([docs](https://code.claude.com/docs/en/terminal-config)) | High: keeps the editor usable and the invisible-character strip is a cheap safety win |
| Draft safety | Claude Code: Ctrl+S stashes the prompt, Up restores the last 10 cleared drafts, a kill ring (Ctrl+Y, Alt+Y) | Cheap; you never lose a long prompt to Ctrl+C |
| Rewind menu | Claude Code `Esc Esc`: restore code and conversation, either alone, or "summarize from here" ([docs](https://code.claude.com/docs/en/checkpointing)) | Psi has `/undo`, `/rewind`, `/tree`. "Summarize from here" is the new idea |
| Side questions | Claude Code `/btw`, Codex `/side`: an answer outside the history, no tools; the pi-btw package has 38k downloads a month | Good with llama-server's prefix cache (whether the cache is reused is unverified) |
| Receipts | Codewhale `/receipts`: files changed, commands run, MCP calls ([repo](https://github.com/codewhale-hq/Codewhale)) | Psi's `/stats` and `/diff` cover most of it |
| Program status | pi 1.1 and Claude Code emit OSC 7501 (working, waiting, done, failed); Codex puts a spinner in the terminal title | A title spinner is cheap; OSC 7501 terminal support is unverified |
| Command palette | Amp replaced slash commands with a palette (2025-10-28); OpenCode and Crush use Ctrl+P | See §7 |

**What pi users install most** ([pi.dev/packages](https://pi.dev/packages), monthly downloads): a todo overlay (178k), ask-user-question (148k), plan review (124k), pi-powerline-footer, a status line with git counts and a context meter (97k), Claude Code-style output (75k), usage display (76k). Notification packages are small (pi-notify about 650).

## 4. Terminal and TUI experience

Psi already does the hard parts (synchronized output, the kitty keyboard flag, focus events). The best next steps are small escape sequences that modern terminals understand and older ones ignore. One startup probe would unlock several of them.

| Upgrade | What it gives you | How | Terminals | Effort |
| --- | --- | --- | --- | --- |
| Word-level diffs | Changed words highlighted inside edit results, not just changed lines | `similar` (already a dependency) with its `inline` feature (`iter_inline_changes`); delta's pairing of similar lines is the model ([similar](https://docs.rs/similar/latest/similar/), [delta](https://github.com/dandavison/delta)) | All | Small |
| Side-by-side diffs | Split view on wide terminals | Crush switches at 140 columns, OpenCode `diff_style: "auto"` ([OpenCode](https://opencode.ai/docs/tui/)); rendered once at the current width | All | Medium |
| Clickable links (OSC 8) | Click a `file:line` in a tool result or a URL in markdown | Write the escapes directly; Psi keeps stripping OSC 8 from untrusted tool output and emits only its own ([spec](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda), [support](https://github.com/Alhadis/OSC8-Adoption)) | kitty, ghostty, wezterm, foot, iTerm2, Alacritty, VTE; others show plain text | Small |
| Notifications | A desktop notice when a long turn ends or an approval waits, only while unfocused | OSC 99 (kitty, foot), OSC 9 or 777 (ghostty, wezterm, foot); `notify-rust` 4.18 as a local fallback ([kitty](https://sw.kovidgoyal.net/kitty/desktop-notifications/), [ghostty](https://ghostty.org/docs/config/reference)) | Most modern terminals; tmux needs passthrough | Small |
| Progress in the tab (OSC 9;4) | A busy indicator in the tab or window while a turn runs | `anstyle-progress` 0.1.4 or the raw sequence | ghostty 1.2+, kitty 0.47+, Windows Terminal | Small |
| Prompt marks (OSC 133) | The terminal's jump-to-prompt walks your turns in scrollback | Mark each committed user prompt; inline Psi can do this, fullscreen agents can't | kitty, ghostty, foot, wezterm, iTerm2 | Small |
| Copy (OSC 52) | `/copy` the last answer or a code block, also over SSH | OSC 52 first, `arboard` 3.7 as fallback (Wayland needs its `wayland-data-control` feature) ([arboard](https://github.com/1Password/arboard)) | kitty, foot, ghostty (write), wezterm (write), tmux with `set-clipboard` | Small |
| Light or dark detection | Code and diff colors that suit the background; live switching | `terminal-colorsaurus` 1.0 (what bat and delta use) reads OSC 11; mode 2031 reports theme changes ([repo](https://github.com/tautropfli/terminal-colorsaurus)) | Most; mode 2031 in foot and ghostty | Small |
| Grapheme widths (mode 2027) | Emoji and joined characters stop misplacing the cursor | Query and enable 2027, measure per grapheme cluster with `unicode-segmentation` (a dependency already) ([background](https://mitchellh.com/writing/grapheme-clusters-in-terminals)) | foot, ghostty, wezterm; kitty segments correctly by default | Small to medium |
| Startup probe | One round trip answers all the questions above | Send each query, then Primary Device Attributes (`CSI c`), which every terminal answers, so there's no timeout | All | Small |
| Images | Show screenshots or plots; paste images | Kitty graphics, sixel (`icy_sixel`), iTerm2; `ratatui-image` 11 as a reference for detection ([repo](https://github.com/ratatui/ratatui-image)). Committing an image into scrollback works in kitty and foot; untested in ghostty and wezterm | kitty, ghostty, foot (sixel), iTerm2, wezterm (partly) | Medium; only worth it with a vision model |
| Math and diagrams | `\alpha` as α; mermaid blocks as box drawing | `unicodeit` 0.2 (small); `merman` or `mermaid-text` (young, test on real output first) | All | Small, medium |

**What not to do:**
- **Mouse capture:** without the alternate screen it breaks native scrolling and selection. OSC 133 gives most of the navigation benefit.
- **Adopting a ratatui widget or reedline:** they own the terminal or a ratatui buffer, so they don't fit Psi's line renderer. Borrow their ideas instead: Ctrl+R history search, fish-style hints, a vi keymap ([reedline](https://github.com/nushell/reedline)).

**Caveats:**
- crossterm hasn't released since 0.29.0 (2025-04), though its repo is active.
- Shift+Enter under the disambiguate-only keyboard flag should be tested in each of your terminals.

## 5. Search and code navigation

Psi doesn't need to download or bundle rg and fd: it can link ripgrep's crates for its own features and check that the binaries are on PATH for the model's bash. The search features you'd notice most are the human ones: a fresh, well-ranked `@` list and a file and grep picker.

**Found in Psi (verified):**
- The `@` completion list is built on the first `@` and never refreshed, so files created during a session never appear (`crates/psi-cli/src/interactive/complete.rs:86`).
- That walk (up to 20,000 files) runs on the UI path.
- The eval image installs `ripgrep` but not `fd`.
- The system prompt names no search tool.

**How others get rg and fd:**

| Agent | Approach |
| --- | --- |
| pi | Looks in `~/.pi/agent/bin`, then PATH; otherwise downloads the latest rg and fd from GitHub releases with no checksum check. fd powers its `@` completion; rg is put on PATH for the bash tool so its "use rg" prompt line works. Read from pi's source (`utils/tools-manager.ts`) |
| Codex CLI | Vendors rg 15.2.0 pinned by sha256 and adds it to PATH; the prompt says "prefer `rg` or `rg --files`". Its TUI file search is the `ignore` and `nucleo` crates in-process ([repo](https://github.com/openai/codex)) |
| Claude Code | Ships its own rg. On macOS and Linux its Grep and Glob tools are now off by default: the model types `grep` and `find`, which run embedded `ugrep` and `bfs` ([docs](https://code.claude.com/docs/en/tools-reference)) |
| OpenCode | System rg, else downloads rg 15.1.0 with a SHA-256 check |
| Gemini CLI | Vendored rg binaries, then PATH, then a JavaScript fallback |
| Crush | rg from PATH; warns and falls back to a Go walk |

**Options for Psi:**
1. **Link the crates for Psi's own features (recommended).**
    - `ignore` is already used.
    - `grep-searcher` 0.1.17 would give the grep tool rg's binary and encoding handling.
    - `nucleo-matcher` 0.3 (Helix, fzf-style scoring, MPL-2.0) or `frizbee` 0.13 (MIT, typo-tolerant, used by atuin and blink.cmp) would replace the hand-written fuzzy scorer.
2. **Check PATH at startup.** If `rg` or `fd` is missing, say so once with the fix (`sudo pacman -S ripgrep fd`). Add `fd-find` to the eval image.
3. **Download or vendor binaries:** not recommended. Runtime downloads solve a Node problem Psi doesn't have, and vendoring only matters if Psi ships to other people.

**Structural search, LSP and indexing.** The H-16 eval showed the 35B doesn't call tools it isn't pointed at, which argues against "pulled" tools and for "pushed" information.
- ast-grep has a Rust API (`ast-grep-core` 0.51, same tree-sitter version as Psi), but a structural search tool the model must choose is unlikely to be called.
- **Pushed diagnostics:** append new errors to `edit` and `write` results. The cheap version, which OpenCode's own docs recommend over LSP, is Psi's existing post-edit check run per file. Experiment.
- **Repo map:** Aider's tree-sitter map with PageRank ([docs](https://aider.chat/docs/repomap.html)) costs about 1K tokens of context per request and changes with the chat, which hurts the prompt cache. No evidence found that it helps small models. Large experiment.
- **Semantic search:** Cursor reports +12.5% accuracy with its own embedding model ([blog](https://cursor.com/blog/semsearch)); Continue deprecated its local-embeddings `@Codebase`. For one GPU and small projects, skip.
- **File watching:** a `notify` watcher brings inotify limits and noise from `target/`. Psi already detects external edits with content hashes. Re-walking when `@` opens covers the human side.

**Tools worth having in the sandbox and image:** `rg`, `fd` (Debian's binary is `fdfind`), `jq`, `sd`, `tokei`, `gh`, `tree`. Speed claims for rg over grep come from vendor blogs, and no measurement of tokens or pass rates was found. The practical argument is smaller output, because rg skips ignored directories.

**Model-facing option (needs an eval):** one prompt line naming `rg` and `rg --files`, as Codex and pi have.

## 6. Lua batteries for extensions

Bake Rust-backed modules into `psi.*` (JSON, exec, HTTP, regex, files) rather than supporting LuaRocks. Most of the crates are already in Psi's binary, including reqwest with TLS. Neovim and Yazi both added HTTP in 2026, which shows plugin authors want it.

**Facts that shape this (verified in the repo):**
- Extensions run in `Lua::new()`, which loads `io` and `os`. So `io.popen` and `os.execute` already bypass the bash approval policy; D-25 accepts this, since extensions are trusted.
- **A hung `io.popen` can't be killed.** The instruction-count hook doesn't interrupt a blocking C call, and Esc doesn't reach it. Async Rust batteries fix this; that's a reason beyond convenience.
- `require` already loads pure-Lua modules placed next to `init.lua`.
- reqwest with rustls, `serde_json`, `regex`, `globset`, `ignore`, tokio's process support and `toml` are already in the build, so the batteries add little to the binary.

**What other hosts ship** (abridged):

| Host | JSON | HTTP | Process | Regex |
| --- | --- | --- | --- | --- |
| Neovim 0.12 | `vim.json` | `vim.net.request` (new, wraps curl) | `vim.system` with timeout | `vim.regex`, LPeg |
| Yazi | `ya.json_*` | `ya.http` (new, reqwest) | `Command` builder, async only | none |
| WezTerm | `wezterm.serde` (JSON, TOML, YAML) | none | `run_child_process` | none |
| Lune (Luau) | `@lune/serde` | `@lune/net` | `@lune/process` | `@lune/regex` (Rust) |
| pi (TypeScript) | native | `fetch` | `pi.exec(cmd, args, {timeout})`, no approval | native |

[Neovim](https://neovim.io/doc/user/lua.html), [Yazi](https://github.com/sxyazi/yazi), [WezTerm](https://wezterm.org/config/lua/), [Lune](https://lune-org.github.io/docs/api-reference/net), [pi](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/extensions.md). Nobody ships SQLite or templating as a core battery.

**Proposed API (a sketch to discuss):**

```lua
psi.json.encode(v, { pretty?, sort_keys? }) ; psi.json.decode(s) ; psi.json.null ; psi.json.array(t)
psi.http.request{ url, method?, headers?, query?, body?, json?, timeout_ms?, max_bytes? }
  -> { status, ok, headers, body, json = fun() }   -- errors only on transport failure (Lune's style)
psi.exec(argv, { cwd?, env?, stdin?, timeout_ms?, max_bytes? }) -> { code, stdout, stderr, killed, truncated }
psi.fs.read / write / exists / stat / list ; psi.fs.glob(pattern, { gitignore = true })
psi.path.join / normalize / relative / basename / dirname / ext
psi.re.compile(pat) -> Regex  -- :is_match :find :captures :gmatch :replace :split (the regex crate)
psi.time.now / format / parse ; psi.hash.sha256 ; psi.base64 ; psi.toml ; psi.env.get
```

All of these would be declared in `psi.toml` (D-14), so the LuaLS stubs, `docs/api.md` and `psi ext check` come for free. Calls yield and look synchronous, as in Lune and Yazi; mlua's async functions already work that way in Psi.

**Safety (to settle in a plan edit):** extensions stay trusted, and the batteries make the policy-respecting path the easy one.
- `psi.http` fails cleanly when `network` is off, with a default timeout, size cap and cancellation.
- `psi.exec` runs inside the same sandbox as bash, and its argv is matched against the always-ask list.
- `psi.fs.write` follows the file tools' rules (no `.psi/` writes; asks outside the workspace).
- A capability manifest waits for §13.

**Not recommended:**
- **LuaRocks and C modules:** they need an unsafe Lua state, exported Lua symbols, a C toolchain, and ABI pinning to the vendored Lua.
- **YAML:** the serde YAML crates are deprecated or unmaintained.
- **SQLite and templating:** defer until something needs them.

| Battery | Crate | Effort |
| --- | --- | --- |
| json | serde_json + mlua `serialize` (in tree) | Small |
| exec | tokio process + the bash runner's sandbox wrapper | Medium |
| http | reqwest 0.13.5 (in tree, rustls) | Medium |
| re | regex 1.13 (in tree) | Small |
| fs, path, glob | ignore, globset (in tree) | Small to medium |
| time, hash, base64, toml, env | jiff or chrono, sha2, base64, toml | Small together |
| async helpers (`psi.async.all`, `psi.sleep`) | mlua coroutines | Medium; wait for a need |

`mlua-batteries` 0.7 (one maintainer, created 2026-02) is a useful design reference, not a dependency. `lune-std` is Luau-only.

## 7. Documentation inside the CLI

Psi's help is already the right shape (embedded markdown, `psi help <topic>`, a prompt line). The cheap wins are finding things faster: search, completions, a topic map for the model. Building a docs sub-agent or bundling cheatsheets isn't worth it.

| Option | Who does it | For Psi | Effort |
| --- | --- | --- | --- |
| Topic-to-file map in the prompt | pi's system prompt maps topics to docs files and says "read the relevant docs and examples in full" before harness changes ([source](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/src/core/system-prompt.ts)) | Add a one-line topic list to Psi's `psi help` prompt line, and the "read in full before writing an extension" rule. Model-facing text, so it needs an eval | Small |
| `psi help --search <word>` | Gemini CLI has a `get_internal_docs` tool that lists and returns its docs ([source](https://geminicli.com/docs/tools/internal-docs)) | A grep over the embedded topics returning topic and line. One bash call finds the answer when the model guesses the wrong topic name. No new tool (schema cost) | Small |
| `/help <topic>` in the TUI | Claude Code `/help`, Gemini `/help` and `/docs`, pi `/hotkeys` ([source](https://code.claude.com/docs/en/interactive-mode)) | Print the topic into scrollback; same text as `psi help <topic>` | Small |
| Command palette (Ctrl+P) | OpenCode (`ctrl+p`, plus a `ctrl+x` leader), Crush (palette lists skills too) ([source](https://opencode.ai/docs/keybinds/)) | A fuzzy list of slash commands, templates, skills and extension commands, reusing the `@path` matcher | Medium |
| Shell completions | `clap_complete` 4.6 `CompleteEnv`; fish users add `COMPLETE=fish psi \| source` ([source](https://docs.rs/clap_complete/latest/clap_complete/env/struct.CompleteEnv.html)) | Completes help topics, models, sessions and eval task names dynamically | Small |
| Man pages | `clap_mangen` 0.2.26 ([source](https://docs.rs/clap_mangen/)) | A `just man` recipe; matters mainly for packaging | Small |
| Docs sub-agent | Claude Code's `claude-code-guide` (runs on Haiku) ([source](https://code.claude.com/docs/en/sub-agents)) | Not worth it: slow on a local model, and the help is small enough to read | Large |
| Library docs for the model | context7 (hosted MCP), offline DevDocs MCP servers ([source](https://glama.ai/mcp/servers/maxwhipw/devdocs-mcp)) | A documented MCP recipe, nothing in core. A skill that teaches `go doc`, `python -m pydoc` and similar before guessing an API | Small (docs) |
| Cheatsheets, terminal renderers | navi, tldr, cheat.sh; glow; mdcat was archived 2026-06-19 ([source](https://github.com/swsnr/mdcat)) | `psi ext example` already fills the role. Mention `psi help x \| glow -` as a tip | Small |

Unknown: whether local models get fewer API-signature errors with offline docs lookups. No measurement found; one eval task would tell.

## 8. Working with other terminal tools

Because Psi is inline, launching a full-screen tool is easy: leave raw mode, clear the input area, run the program on the terminal, restore. Yazi, nvim and lazygit use the alternate screen, so Psi's scrollback survives them. One "suspend and launch" routine unlocks most of this section.

| Option | Who does it | For Psi | Effort |
| --- | --- | --- | --- |
| Edit the prompt in `$EDITOR` (Ctrl+G) | pi (`app.editor.external`, `$VISUAL` then `$EDITOR`), Claude Code (Ctrl+G or Ctrl+X Ctrl+E), Codex (Ctrl+G, Dec 2025) ([pi](https://pi.dev/docs/latest/keybindings), [Codex](https://github.com/openai/codex/issues/7606)) | pi's action id with Ctrl+G, so `keybindings.json` stays pi-compatible. Draft goes to a temp `.md`, comes back into the input | Small |
| File picker (yazi, fzf) | `yazi --chooser-file=<tmp>` writes the picked paths; agent-tui.yazi, tmux setups insert `@path` into agent panes ([yazi tips](https://yazi-rs.github.io/docs/tips), [agent-tui.yazi](https://github.com/tr1v3r/agent-tui.yazi)) | A `picker` setting (e.g. `yazi --chooser-file={out}` or `fzf -m`) on a key; each line becomes `@path`. `fzf --tmux` inside tmux | Small |
| Diff viewer for `/diff` | delta as a pager, difftastic as `diff.external` ([delta](https://dandavison.github.io/delta/configuration.html), [difftastic](https://difftastic.wilfred.me.uk/git.html)) | A `diff_pager` setting; `/diff` pipes a unified diff through it with the TUI suspended. Built-in stays the default | Small |
| lazygit and other TUIs | lazygit custom commands run AI commit tools ([commit-oracle](https://github.com/tfriedel/commit-oracle)) | `/run <cmd>` (or check whether `!lazygit` can hand over the terminal). Checkpoints already cover undo; lazygit is for review and commits | Small |
| Desktop notifications | Ghostty handles OSC 9 and 777; kitty OSC 99 and `kitten notify`; Claude Code users patch it in with hooks ([kitty](https://sw.kovidgoyal.net/kitty/desktop-notifications/), [issue](https://github.com/anthropics/claude-code/issues/41484)) | A `notify` setting: OSC 9 (OSC 99 on kitty) when a long run ends or an approval waits; tmux passthrough | Small |
| Prompt marks (OSC 133) | Ghostty, kitty, WezTerm, iTerm2 jump between shell prompts; Claude Code users ask for it | Psi commits into real scrollback, so marking each user prompt makes the terminal's jump-to-prompt walk the conversation. Full-screen TUIs can't do this | Small |
| tmux and zellij | `display-popup` pickers over agent panes ([claude-tmux](https://github.com/nielsgroen/claude-tmux)); zellij floating panes ([docs](https://zellij.dev/documentation/zellij-run-and-edit.html)) | Set the pane title (OSC 2) to `psi: working` or `psi: waiting` for status tools. Under zellij, open the picker, editor and diff in a floating pane | Small |
| Shell history (atuin) | `atuin hook install pi` writes a pi extension recording each bash call as `author=pi` ([source](https://docs.atuin.sh/main/guide/agent-hooks/)) | An example Lua extension doing the same with `author=psi` | Small |
| direnv and mise | Agents' non-interactive shells miss direnv's hook; people snapshot `direnv export` in hooks ([agent-profile](https://github.com/crisog/agent-profile)) | An `env_loader` setting: run `direnv export json` or `mise env` with a timeout at start and on cwd change, merge into bash's environment | Medium |
| Neovim | claudecode.nvim speaks Claude Code's IDE bridge (a WebSocket MCP server, a lock file in `~/.claude/ide/`, tools like `openDiff`, `getCurrentSelection`) ([protocol](https://github.com/coder/claudecode.nvim/blob/main/PROTOCOL.md)); pi has several community nvim plugins over its RPC mode | Either connect to that bridge as a client, so claudecode.nvim works with Psi as is (unverified that it accepts other clients), or build RPC mode and a small plugin | Medium, or large |
| ACP (Agent Client Protocol) | Zed, codecompanion.nvim, avante.nvim; pi reaches them through adapters over `pi --mode rpc` ([codecompanion](https://codecompanion.olimorris.dev/agent-client-protocol), [pi-acp](https://github.com/DxTa/pi-acp)) | RPC mode first, then an ACP adapter: one protocol for Zed and Neovim | Large |
| Terminal remote control | `wezterm cli split-pane`, `kitten @ launch` | Optional "open in a split" backends; low priority | Small each |
| zoxide, Helix, bat | No agent-specific integration found; Helix has no stable plugins | A fish function (`function pz; zi $argv && psi; end`) in the docs; nothing else | None |

## 9. Sessions, history and local-model quality of life

Psi's session tree is already ahead of most agents. What's missing is finding things (prompt history search, session search) and seeing what a slow local model is doing (speed, prompt processing, context use).

| Option | Who does it | For Psi | Effort |
| --- | --- | --- | --- |
| Prompt history search (Ctrl+R) | Claude Code (per directory, duplicates collapsed), Codex, Goose, Aider | Search over the editor's history, kept per project | Small to medium |
| Session picker with search | Claude Code (search, preview, rename, filter by branch), pi `/resume` (searches message text, rename, delete) | Full-text search in `/resume`, a preview, rename (reusing `/label`) | Medium |
| Trim before resuming | Goose `--resume --edit` opens the conversation in `$EDITOR` | Interesting, but Psi's `/tree` and `/fork` already cover going back | Skip |
| Transcript to pager or editor | Claude Code transcript mode `v` opens it in `$EDITOR` | `/transcript`: write the session as markdown and open `$PAGER` or `$EDITOR`. Inline Psi already has the terminal's own search | Small |
| Per-turn diffs | Claude Code `/diff` with per-turn views and "ask about this file" | Psi's checkpoints already hold each turn; add turn selection to `/diff` | Small to medium |
| Status line segments | Claude Code (a script with session JSON), Codex (an ordered item list), pi-powerline-footer (git counts, context color at 70% and 90%) | Built-in segments plus a Lua hook. For llama-server: tokens per second, prompt-processing progress, context used, cache reuse (llama-server reports timings; Psi records them) | Small to medium |
| Terminal title | Codex `tui.terminal_title`, OSC 7501 in pi and Claude Code | `psi: working` or `psi: waiting` in the title; tmux and zellij tools can read it | Small |
| HTML export | pi `/export` | Already listed as wanted, not built (Phase 7 close) | Medium |

## 10. Shortlist and open questions

Start with the quick wins: they're small, human-side, need no eval run, and you'd feel them every day. Lua batteries are the main medium-sized project and need a plan edit first.

**Quick wins** (small, no eval):

| # | Item | Section |
| --- | --- | --- |
| 1 | Edit the prompt in `$EDITOR` with Ctrl+G (pi's `app.editor.external` id) | §8 |
| 2 | Notifications when unfocused (OSC 99, 9, 777), progress in the tab (OSC 9;4), a title spinner | §4, §8 |
| 3 | Fix the stale `@` list and move its walk off the UI path; rank with nucleo-matcher or frizbee | §5 |
| 4 | Word-level diff highlighting with `similar` | §4 |
| 5 | Clickable `file:line` links and URLs (OSC 8) | §4 |
| 6 | OSC 133 prompt marks: jump between your turns in scrollback | §4, §8 |
| 7 | Ctrl+R prompt history search and a draft stash | §3, §9 |
| 8 | Paste collapse and an invisible-Unicode strip | §3 |
| 9 | `/copy` via OSC 52 | §4 |
| 10 | rg and fd startup check with the pacman hint; `fd-find` in the eval image | §5 |
| 11 | Shell completions for fish (`clap_complete`), `/help <topic>`, `psi help --search` | §7 |

**Medium projects:**
- **Lua batteries** (`psi.json`, `psi.exec`, `psi.http`, `psi.re`, `psi.fs`): needs a D-xx for the exec and HTTP safety rules (§6).
- **One suspend-and-launch routine**, which enables a file picker (yazi or fzf), a `diff_pager` (delta, difftastic) and `/run lazygit` (§8).
- **A status line** with tokens per second, prompt-processing progress and a context meter (§9).
- **Session search** in `/resume`, and per-turn `/diff` (§9).
- **A Ctrl+P command palette** (§7).
- **Light or dark detection and grapheme widths** through one startup probe (§4).
- **direnv or mise environment loading** for bash (§8).

**Experiments** (model-facing; one eval run each):
- A topic map in the `psi help` prompt line (§7).
- A prompt line naming `rg` (§5).
- Pushed diagnostics after edits (§5).
- A tree-sitter repo map (§5).

**Later or skip:**
- **Later:** images (only with a vision model), vim mode (Ctrl+G covers most of it), the Claude Code IDE-bridge client and ACP (after RPC mode).
- **Skip:** mouse capture, LuaRocks, runtime binary downloads, embeddings indexes, a docs sub-agent.

**Open questions for you:**
1. **Inline or fullscreen?** Decided 2026-11-02: stay inline for now, revisit later.
    - **Why inline:** it keeps tmux copy mode and search, the conversation stays on screen after Psi exits, prompt marks become possible, and it's built.
    - **What it costs:** printed output is frozen (no collapsing, no re-layout on resize), and long sessions pass tmux's default 2,000-line `history-limit`.
    - **Later, if wanted:** a `/transcript` view that opens a temporary fullscreen screen to browse, search and expand the session, then returns to scrollback (like Claude Code's transcript mode). Or a fullscreen option.
2. **Terminals:** answered 2026-11-02. Ghostty most of the time, almost always inside tmux.
    - Every escape sequence above has to pass through tmux: OSC 8 links need tmux 3.4+ with `terminal-features`, and notifications need `allow-passthrough on`.
    - Prompt marks (OSC 133) and progress (OSC 9;4) through tmux are unchecked.
    - So test each item in ghostty through tmux first.
3. **Lua batteries safety:** decided 2026-11-02, recorded as D-45 in `psi-plan.md` §9.4.
    - `psi.exec` goes through the sandbox and the always-ask list.
    - `io.popen` and `os.execute` are removed in the same change.
    - An extension that must leave the sandbox gets a per-extension `unsandboxed` opt-out, set in the user's settings, not in its own manifest.
    - File writes through `io` and `os` stay until `psi.fs`.
4. **Editor:** answered 2026-11-02. Neovim, in a tmux pane next to Psi. The editor bridge is wanted later; recorded in the `psi-plan.md` §15 backlog with RPC mode and ACP. The inline decision is in that backlog too.
5. **Vision:** answered 2026-11-02: no, or not soon. Images in the terminal and image paste stay in "later".
6. **Distribution:** answered 2026-11-02: only the owner for now, open to sharing if it proves useful to others. rg and fd come from pacman with a startup check; bundled binaries, man pages and packaging wait until sharing is on the table (with §13).

## Sources

Linked inline in each section. The pi findings come from its source and docs (`packages/coding-agent`, local clone at commit 35dfa7573, and [pi.dev](https://pi.dev/docs/latest/keybindings)). Crate versions are from crates.io on 2026-11-02.
