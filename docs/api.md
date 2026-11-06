# Psi extension API

<!-- Generated from crates/psi-ext-api/api by build.rs; do not edit. -->

An extension's `setup(psi)` gets the `psi` table below. Types use the API's
grammar: `T?` is optional, `T[]` a list, `{K: V}` a map, `"a"|"b"` one of those
strings and `function(name: T): R` a function; other names are the types at the
end. Editor completion: the LuaLS stubs in `docs/api/psi.lua`.

## Functions

An argument of the wrong type is an error naming the function and argument.
Functions marked *async* wait for their answer, suspending the calling handler
(not psi).

### psi

#### psi.on

```
psi.on(event: EventName, handler: function(ev: table, ctx: Context): any)
```

Subscribe to an event. Gate and chain events use the handler's return value (see the event); errors are reported and never crash psi, and a failing `tool_call` handler blocks the call.

| Param | Type | Optional |
|---|---|---|
| `event` | `EventName` |  |
| `handler` | `function(ev: table, ctx: Context): any` |  |

#### psi.register_tool

```
psi.register_tool(spec: ToolSpec)
```

Add a tool the model can call. It goes through the same policy as built-in tools: `always_ask` makes every call ask.

| Param | Type | Optional |
|---|---|---|
| `spec` | `ToolSpec` |  |

#### psi.register_command

```
psi.register_command(name: string, spec: CommandSpec)
```

Add a slash command (`/name args`) to the interactive mode.

| Param | Type | Optional |
|---|---|---|
| `name` | `string` |  |
| `spec` | `CommandSpec` |  |

#### psi.register_shortcut

```
psi.register_shortcut(key: string, spec: ShortcutSpec)
```

Bind a key (like "ctrl+t") in the interactive mode.

| Param | Type | Optional |
|---|---|---|
| `key` | `string` |  |
| `spec` | `ShortcutSpec` |  |

#### psi.register_flag

```
psi.register_flag(name: string, spec: FlagSpec)
```

Declare a flag set on the command line with `--ext-flag name=value`.

| Param | Type | Optional |
|---|---|---|
| `name` | `string` |  |
| `spec` | `FlagSpec` |  |

#### psi.register_provider

```
psi.register_provider(name: string, spec: ProviderSpec)
```

Add an OpenAI-compatible endpoint and its models; pick one with `psi --model name/id` (or the id alone). Registering a name again replaces it.

| Param | Type | Optional |
|---|---|---|
| `name` | `string` |  |
| `spec` | `ProviderSpec` |  |

#### psi.get_flag

```
psi.get_flag(name: string): any
```

A flag's value: the command line's, else its default.

| Param | Type | Optional |
|---|---|---|
| `name` | `string` |  |

Returns `any`.

#### psi.register_tool_renderer

```
psi.register_tool_renderer(tool: string, render: function(call: ToolRenderCall, width: integer): string[])
```

Draw a tool's committed call yourself: return the lines (style them with psi.style).

| Param | Type | Optional |
|---|---|---|
| `tool` | `string` |  |
| `render` | `function(call: ToolRenderCall, width: integer): string[]` |  |

#### psi.send_user_message

```
psi.send_user_message(text: string, opts: SendOptions?)
```

Send a message as the user. While the agent runs it steers the run, or waits for the run to end with `deliver = "follow_up"`; otherwise it starts a run. The text is always a prompt: a leading `/` or `!` runs no command or shell.

| Param | Type | Optional |
|---|---|---|
| `text` | `string` |  |
| `opts` | `SendOptions?` | yes |

#### psi.send_message

```
psi.send_message(custom_type: string, text: string)
```

Add a message from the extension that the model sees with the next request (a `custom_message` entry).

| Param | Type | Optional |
|---|---|---|
| `custom_type` | `string` |  |
| `text` | `string` |  |

#### psi.append_entry

```
psi.append_entry(custom_type: string, data: any)
```

Store data in the session (a `custom` entry, never sent to the model).

| Param | Type | Optional |
|---|---|---|
| `custom_type` | `string` |  |
| `data` | `any` |  |

#### psi.entries

```
psi.entries(custom_type: string): table[]
```

The current branch's entries of one custom type, oldest first, for state kept with append_entry.

| Param | Type | Optional |
|---|---|---|
| `custom_type` | `string` |  |

Returns `table[]`.

#### psi.set_active_tools

```
psi.set_active_tools(names: string[])
```

Choose which tools the model is offered from the next request on.

| Param | Type | Optional |
|---|---|---|
| `names` | `string[]` |  |

#### psi.get_active_tools

```
psi.get_active_tools(): string[]
```

The names of the tools the model is offered.

Returns `string[]`.

#### psi.execute_tool

```
psi.execute_tool(name: string, args: table): ToolResult -- async
```

Run a tool as the model would, through the policy (it may ask the user). Also `ctx.execute_tool`.

| Param | Type | Optional |
|---|---|---|
| `name` | `string` |  |
| `args` | `table` |  |

Returns `ToolResult`.

#### psi.log

```
psi.log(message: string)
```

Write to psi's log (PSI_LOG=info shows it); `psi ext test` prints it.

| Param | Type | Optional |
|---|---|---|
| `message` | `string` |  |

#### psi.exec

```
psi.exec(argv: string[], opts: ExecOptions?): ExecResult -- async
```

Run a program and wait for it, inside the same boundary as bash, with bash's environment. Commands on the always-ask list ask the user (the card names this extension); headless runs refuse them. `{"sh", "-c", script}` is checked like a bash command. Errors when refused, denied or the program can't start; a non-zero exit is a result. Esc stops it. Use it instead of io.popen and os.execute, which extensions don't have.

| Param | Type | Optional |
|---|---|---|
| `argv` | `string[]` |  |
| `opts` | `ExecOptions?` | yes |

Returns `ExecResult`.

### psi.events

#### psi.events.emit

```
psi.events.emit(name: string, data: any)
```

Send an event to every extension's events.on handlers for that name.

| Param | Type | Optional |
|---|---|---|
| `name` | `string` |  |
| `data` | `any` |  |

#### psi.events.on

```
psi.events.on(name: string, handler: function(data: any))
```

Listen to events.emit calls from any extension.

| Param | Type | Optional |
|---|---|---|
| `name` | `string` |  |
| `handler` | `function(data: any)` |  |

### psi.ui

#### psi.ui.notify

```
psi.ui.notify(message: string, level: "info"|"warning"|"error"?)
```

Show a one-line message. In headless runs it goes to the log.

| Param | Type | Optional |
|---|---|---|
| `message` | `string` |  |
| `level` | `"info"\|"warning"\|"error"?` | yes |

#### psi.ui.confirm

```
psi.ui.confirm(title: string, message: string): boolean -- async
```

Ask the user a yes/no question. Returns false in headless modes.

| Param | Type | Optional |
|---|---|---|
| `title` | `string` |  |
| `message` | `string` |  |

Returns `boolean`.

#### psi.ui.select

```
psi.ui.select(title: string, options: string[]): string? -- async
```

Let the user pick one option. Returns nil if they cancel, and in headless modes.

| Param | Type | Optional |
|---|---|---|
| `title` | `string` |  |
| `options` | `string[]` |  |

Returns `string?`.

#### psi.ui.input

```
psi.ui.input(title: string, placeholder: string?): string? -- async
```

Ask the user for a line of text. Returns nil if they cancel, and in headless modes.

| Param | Type | Optional |
|---|---|---|
| `title` | `string` |  |
| `placeholder` | `string?` | yes |

Returns `string?`.

#### psi.ui.status

```
psi.ui.status(key: string, text: string?)
```

Set (or clear, with nil) a short item on the status line.

| Param | Type | Optional |
|---|---|---|
| `key` | `string` |  |
| `text` | `string?` | yes |

#### psi.ui.widget

```
psi.ui.widget(slot: string, lines: string[]?)
```

Show lines above the input box under a slot name (nil removes them).

| Param | Type | Optional |
|---|---|---|
| `slot` | `string` |  |
| `lines` | `string[]?` | yes |

#### psi.ui.custom

```
psi.ui.custom(component: Component): any -- async
```

Show a component under the input box until its handle_input returns a value, which ui.custom returns. Returns nil in headless modes.

| Param | Type | Optional |
|---|---|---|
| `component` | `Component` |  |

Returns `any`.

### psi.style

#### psi.style.color

```
psi.style.color(color: string, text: string): string
```

Text in one of the terminal's 16 colors: black, red, green, yellow, blue, magenta, cyan, white, and bright_<color>.

| Param | Type | Optional |
|---|---|---|
| `color` | `string` |  |
| `text` | `string` |  |

Returns `string`.

#### psi.style.bold

```
psi.style.bold(text: string): string
```

Bold text.

| Param | Type | Optional |
|---|---|---|
| `text` | `string` |  |

Returns `string`.

#### psi.style.dim

```
psi.style.dim(text: string): string
```

Dimmed text.

| Param | Type | Optional |
|---|---|---|
| `text` | `string` |  |

Returns `string`.

### psi.bash

#### psi.bash.writes

```
psi.bash.writes(command: string): string?
```

Why a bash command may change something, or nil when it only reads: every program in it (through pipes, `&&`, substitutions and `sh -c`) is a known reader such as `cat`, `grep` or `git diff`, and nothing is redirected into a file. Plan mode uses the same check. A `tool_call` gate can block writes to a file and let reads of it through: `if cmd:find("docs/words.md", 1, true) and psi.bash.writes(cmd) then ... end`.

| Param | Type | Optional |
|---|---|---|
| `command` | `string` |  |

Returns `string?`.

### psi.http

#### psi.http.request

```
psi.http.request(req: HttpRequest): HttpResponse -- async
```

Make an HTTP or HTTPS request. Errors when the network is off (the `network` setting), on a bad URL, or when the connection fails or times out; any status is a response. Esc stops it.

| Param | Type | Optional |
|---|---|---|
| `req` | `HttpRequest` |  |

Returns `HttpResponse`.

#### psi.http.get

```
psi.http.get(url: string): HttpResponse -- async
```

A GET request: psi.http.request{ url = url }.

| Param | Type | Optional |
|---|---|---|
| `url` | `string` |  |

Returns `HttpResponse`.

### psi.json

#### psi.json.encode

```
psi.json.encode(value: any, opts: JsonOptions?): string
```

A value as JSON. Keys come out sorted. An empty table is {} unless marked with psi.json.array; functions are an error.

| Param | Type | Optional |
|---|---|---|
| `value` | `any` |  |
| `opts` | `JsonOptions?` | yes |

Returns `string`.

#### psi.json.decode

```
psi.json.decode(text: string): any
```

Parses JSON. null becomes psi.json.null() (so arrays keep their length), and arrays decode marked as arrays.

| Param | Type | Optional |
|---|---|---|
| `text` | `string` |  |

Returns `any`.

#### psi.json.null

```
psi.json.null(): any
```

The value JSON null decodes to; put it in a table to encode a null.

Returns `any`.

#### psi.json.array

```
psi.json.array(t: table): table
```

Marks a table as an array, so it encodes as [] even when empty. Returns the table.

| Param | Type | Optional |
|---|---|---|
| `t` | `table` |  |

Returns `table`.

### psi.re

#### psi.re.is_match

```
psi.re.is_match(pattern: string, text: string): boolean
```

Whether `pattern` matches anywhere in `text`. Patterns use Rust regex syntax (Perl-like, without lookaround or backreferences); (?i) ignores case.

| Param | Type | Optional |
|---|---|---|
| `pattern` | `string` |  |
| `text` | `string` |  |

Returns `boolean`.

#### psi.re.find

```
psi.re.find(pattern: string, text: string): Match?
```

The first match, or nil.

| Param | Type | Optional |
|---|---|---|
| `pattern` | `string` |  |
| `text` | `string` |  |

Returns `Match?`.

#### psi.re.find_all

```
psi.re.find_all(pattern: string, text: string): Match[]
```

Every match that doesn't overlap the one before, in order.

| Param | Type | Optional |
|---|---|---|
| `pattern` | `string` |  |
| `text` | `string` |  |

Returns `Match[]`.

#### psi.re.replace

```
psi.re.replace(pattern: string, text: string, replacement: string, limit: integer?): string
```

`text` with matches replaced: all of them, or the first `limit`. In `replacement`, $1 and ${name} are capture groups and $$ is a dollar sign.

| Param | Type | Optional |
|---|---|---|
| `pattern` | `string` |  |
| `text` | `string` |  |
| `replacement` | `string` |  |
| `limit` | `integer?` | yes |

Returns `string`.

#### psi.re.split

```
psi.re.split(pattern: string, text: string): string[]
```

`text` split at each match.

| Param | Type | Optional |
|---|---|---|
| `pattern` | `string` |  |
| `text` | `string` |  |

Returns `string[]`.

#### psi.re.escape

```
psi.re.escape(s: string): string
```

`s` with every regex special character escaped, so a pattern matches it literally: `"^" .. psi.re.escape(dir)`.

| Param | Type | Optional |
|---|---|---|
| `s` | `string` |  |

Returns `string`.

### psi.path

#### psi.path.join

```
psi.path.join(base: string, part: string): string
```

`part` under `base`; an absolute `part` replaces `base`.

| Param | Type | Optional |
|---|---|---|
| `base` | `string` |  |
| `part` | `string` |  |

Returns `string`.

#### psi.path.normalize

```
psi.path.normalize(p: string): string
```

The path with `.` and `..` resolved, without looking at the disk.

| Param | Type | Optional |
|---|---|---|
| `p` | `string` |  |

Returns `string`.

#### psi.path.absolute

```
psi.path.absolute(p: string): string
```

The path from the working directory, normalized.

| Param | Type | Optional |
|---|---|---|
| `p` | `string` |  |

Returns `string`.

#### psi.path.relative

```
psi.path.relative(p: string, base: string?): string
```

`p` relative to `base` (default: the working directory), with `..` where needed; a result starting with `..` lies outside `base`.

| Param | Type | Optional |
|---|---|---|
| `p` | `string` |  |
| `base` | `string?` | yes |

Returns `string`.

#### psi.path.basename

```
psi.path.basename(p: string): string
```

The last part: "c.txt" for "a/b/c.txt".

| Param | Type | Optional |
|---|---|---|
| `p` | `string` |  |

Returns `string`.

#### psi.path.dirname

```
psi.path.dirname(p: string): string
```

Everything but the last part: "a/b" for "a/b/c.txt", "." for "c.txt".

| Param | Type | Optional |
|---|---|---|
| `p` | `string` |  |

Returns `string`.

#### psi.path.ext

```
psi.path.ext(p: string): string
```

The extension without the dot: "txt" for "a/b/c.txt", "" when there is none.

| Param | Type | Optional |
|---|---|---|
| `p` | `string` |  |

Returns `string`.

### psi.fs

#### psi.fs.read

```
psi.fs.read(path: string): string
```

A file's contents. Errors when it can't be read.

| Param | Type | Optional |
|---|---|---|
| `path` | `string` |  |

Returns `string`.

#### psi.fs.write

```
psi.fs.write(path: string, data: string)
```

Writes a file, creating its directories. Inside the workspace, the next checkpoint captures the change.

| Param | Type | Optional |
|---|---|---|
| `path` | `string` |  |
| `data` | `string` |  |

#### psi.fs.exists

```
psi.fs.exists(path: string): boolean
```

Whether the path exists.

| Param | Type | Optional |
|---|---|---|
| `path` | `string` |  |

Returns `boolean`.

#### psi.fs.stat

```
psi.fs.stat(path: string): Stat?
```

What the path is (following links), or nil when it doesn't exist.

| Param | Type | Optional |
|---|---|---|
| `path` | `string` |  |

Returns `Stat?`.

#### psi.fs.list

```
psi.fs.list(dir: string): string[]
```

The names in a directory, sorted.

| Param | Type | Optional |
|---|---|---|
| `dir` | `string` |  |

Returns `string[]`.

#### psi.fs.glob

```
psi.fs.glob(pattern: string, opts: GlobOptions?): string[]
```

Files matching a glob like "src/**/*.rs" (`*` stays within one directory), relative to the working directory or `opts.cwd`, sorted. Skips what .gitignore ignores, hidden files and .git, unless `gitignore = false`.

| Param | Type | Optional |
|---|---|---|
| `pattern` | `string` |  |
| `opts` | `GlobOptions?` | yes |

Returns `string[]`.

### psi.toml

#### psi.toml.decode

```
psi.toml.decode(text: string): table
```

Parses a TOML document into a table; dates become strings.

| Param | Type | Optional |
|---|---|---|
| `text` | `string` |  |

Returns `table`.

#### psi.toml.encode

```
psi.toml.encode(t: table): string
```

A table as a TOML document.

| Param | Type | Optional |
|---|---|---|
| `t` | `table` |  |

Returns `string`.

### psi.base64

#### psi.base64.encode

```
psi.base64.encode(data: string): string
```

Standard base64, with padding.

| Param | Type | Optional |
|---|---|---|
| `data` | `string` |  |

Returns `string`.

#### psi.base64.decode

```
psi.base64.decode(text: string): string
```

Decodes standard base64. Errors on anything else.

| Param | Type | Optional |
|---|---|---|
| `text` | `string` |  |

Returns `string`.

### psi.hash

#### psi.hash.sha256

```
psi.hash.sha256(data: string): string
```

The SHA-256 of the data, in lowercase hex.

| Param | Type | Optional |
|---|---|---|
| `data` | `string` |  |

Returns `string`.

### psi.time

#### psi.time.now

```
psi.time.now(): number
```

Seconds since 1970 (UTC), with a fraction.

Returns `number`.

#### psi.time.format

```
psi.time.format(t: number?, fmt: string?): string
```

A time in local time, with strftime codes (default "%Y-%m-%dT%H:%M:%S%:z"); `t` defaults to now.

| Param | Type | Optional |
|---|---|---|
| `t` | `number?` | yes |
| `fmt` | `string?` | yes |

Returns `string`.

#### psi.time.parse

```
psi.time.parse(text: string, fmt: string?): number?
```

Seconds since 1970 for a time written as RFC 3339 ("2026-11-02T10:00:00Z"), or with strftime codes in `fmt` (local time unless it has %z). nil when it doesn't parse.

| Param | Type | Optional |
|---|---|---|
| `text` | `string` |  |
| `fmt` | `string?` | yes |

Returns `number?`.

## Events

Subscribe with `psi.on(event, function(ev, ctx) ... end)`; `ev` is the payload and `ctx` a [`Context`](#context). Kinds:

- **notify**: Handlers are told; their return values are ignored.
- **chain**: Handlers run in turn; each may return a new value, which the next handler sees.
- **gate**: Any handler can block it; a handler that fails blocks it too.

| Event | Kind | Payload | Result | Description |
|---|---|---|---|---|
| `session_start` | notify | `SessionEvent` |  | The session is open (also after /new, /resume and /reload). |
| `session_shutdown` | notify | `SessionEvent` |  | psi is closing the session. |
| `session_before_compact` | gate | `CompactEvent` | `GateResult?` | Old turns are about to be summarized or dropped to fit the context window. Return { cancel = true } to stop it. |
| `input` | chain | `InputEvent` | `InputResult?` | The user submitted text that is not a command (extension commands run first). Return { text = ... } to change it, or { handled = true } to consume it; skills and prompt templates expand after. |
| `before_agent_start` | chain | `AgentStartEvent` | `AgentStartResult?` | A run is about to start. Return { system = ... } to add text to the end of the system prompt for this run. |
| `agent_start` | notify | `AgentStartEvent` |  | A run started. |
| `agent_end` | notify | `AgentEndEvent` |  | A run ended. |
| `turn_start` | notify | `TurnEvent` |  | A model request is about to be sent. |
| `turn_end` | notify | `TurnEvent` |  | A turn (the request and its tool calls) finished. |
| `context` | notify | `ContextEvent` |  | The messages about to be sent to the model (read-only). |
| `message_start` | notify | `TurnEvent` |  | The model started answering. |
| `message_update` | notify | `MessageUpdateEvent` |  | Streamed answer text. |
| `message_end` | notify | `MessageEndEvent` |  | The model's answer is complete. |
| `model_stats` | notify | `ModelStatsEvent` |  | A model response finished: its tokens and speed, for a status line segment (psi.ui.status). |
| `tool_call` | gate | `ToolCallEvent` | `ToolCallResult?` | A tool call is about to be checked and run. Return { block = true, reason = ... } to refuse it, or { args = {...} } to rewrite its arguments (the rewrite is checked by the policy and recorded). |
| `tool_execution_update` | notify | `ToolCallEvent` |  | A tool call is running. |
| `tool_result` | chain | `ToolResultEvent` | `ToolResult?` | A tool call finished. Return { text = ..., is_error = ... } to change what the model sees. |
| `policy_decision` | notify | `PolicyEvent` |  | The policy asked, refused, or the user answered. |
| `guard_event` | notify | `GuardEvent` |  | A guard fired (a loop, an error streak, a failing checkpoint). |

## Types

### Context

The second argument of every handler.

| Field | Type | Description |
|---|---|---|
| `cwd` | `string` | The working directory |
| `mode` | `"interactive"\|"print"\|"json"\|"test"` | How psi runs |
| `has_ui` | `boolean` | Whether a person can answer ui calls |
| `session_file` | `string?` | The session file |
| `ui` | `table` | psi.ui |
| `execute_tool` | `function(name: string, args: table): ToolResult` | psi.execute_tool |

### ToolSpec

| Field | Type | Description |
|---|---|---|
| `name` | `string` | Lowercase, with _ between words |
| `description` | `string` | What it does and when to use it, for the model |
| `params` | `{string: ParamSpec}` | The arguments |
| `required` | `string[]?` | Required argument names (default: all) |
| `annotations` | `ToolAnnotations?` |  |
| `always_ask` | `boolean?` | Every call asks the user |
| `execute` | `function(args: table, ctx: Context): string\|ToolResult` | Runs a call; an error becomes a failed result |

### ParamSpec

| Field | Type | Description |
|---|---|---|
| `type` | `"string"\|"integer"\|"number"\|"boolean"\|"array"\|"object"` |  |
| `description` | `string` |  |

### ToolAnnotations

| Field | Type | Description |
|---|---|---|
| `read_only` | `boolean?` | Only reads: may run in parallel, and no checkpoint follows |
| `destructive` | `boolean?` |  |
| `open_world` | `boolean?` | Reaches outside the workspace (network, other systems) |

### ToolResult

| Field | Type | Description |
|---|---|---|
| `text` | `string` |  |
| `is_error` | `boolean?` |  |

### CommandSpec

| Field | Type | Description |
|---|---|---|
| `description` | `string` | Shown in /help and completion |
| `handler` | `function(args: string, ctx: Context)` |  |

### ProviderSpec

| Field | Type | Description |
|---|---|---|
| `base_url` | `string` | The OpenAI-compatible API, like http://127.0.0.1:8080/v1 |
| `api_key` | `string?` | Sent as a bearer token; "$NAME" reads the environment variable NAME |
| `headers` | `{string: string}?` | More request headers; values take "$NAME" too |
| `models` | `ProviderModel[]` | The models it serves |

### ProviderModel

| Field | Type | Description |
|---|---|---|
| `id` | `string` | The model name sent in requests |
| `context_window` | `integer?` | Tokens; default: what the server's /props says |
| `max_output` | `integer?` | Tokens per answer (default 8192) |
| `enable_thinking` | `boolean?` | Sent as chat_template_kwargs.enable_thinking |

### ShortcutSpec

| Field | Type | Description |
|---|---|---|
| `description` | `string` |  |
| `handler` | `function(ctx: Context)` |  |

### FlagSpec

| Field | Type | Description |
|---|---|---|
| `description` | `string` |  |
| `default` | `any` |  |

### SendOptions

| Field | Type | Description |
|---|---|---|
| `deliver` | `"steer"\|"follow_up"?` | While a run is going (default steer) |

### ToolRenderCall

| Field | Type | Description |
|---|---|---|
| `name` | `string` |  |
| `args` | `table` |  |
| `result` | `string` |  |
| `is_error` | `boolean` |  |

### Component

A custom UI component (pi-tui parity), drawn under the input box under a deadline.

| Field | Type | Description |
|---|---|---|
| `render` | `function(width: integer): string[]` | The lines to show |
| `handle_input` | `function(key: string): any` | A key like "enter", "up", "a"; a non-nil return closes it |
| `dispose` | `function()?` |  |

### SessionEvent

| Field | Type | Description |
|---|---|---|
| `session_file` | `string?` |  |

### CompactEvent

| Field | Type | Description |
|---|---|---|
| `tokens` | `integer` | The context size, in tokens |
| `reason` | `string?` | "summary" (past the trigger) or "overflow" (the server refused the request) |

### GateResult

| Field | Type | Description |
|---|---|---|
| `cancel` | `boolean?` |  |

### InputEvent

| Field | Type | Description |
|---|---|---|
| `text` | `string` | As typed: before /skill: and prompt template expansion |
| `source` | `"interactive"\|"print"\|"extension"` | Typed in the TUI, the headless prompt (-p or --json), or psi.send_user_message |

### InputResult

| Field | Type | Description |
|---|---|---|
| `text` | `string?` |  |
| `handled` | `boolean?` |  |

### AgentStartEvent

| Field | Type | Description |
|---|---|---|
| `prompt` | `string` |  |

### AgentStartResult

| Field | Type | Description |
|---|---|---|
| `system` | `string?` | Appended to the system prompt for this run |

### AgentEndEvent

| Field | Type | Description |
|---|---|---|
| `stop` | `string` | done, budget, guard, aborted or failed |
| `text` | `string` | The last answer |

### TurnEvent

| Field | Type | Description |
|---|---|---|
| `turn` | `integer` |  |

### ContextEvent

| Field | Type | Description |
|---|---|---|
| `messages` | `table[]` | In OpenAI chat format |

### MessageUpdateEvent

| Field | Type | Description |
|---|---|---|
| `text` | `string` | The new text |

### MessageEndEvent

| Field | Type | Description |
|---|---|---|
| `text` | `string` |  |
| `tool_calls` | `table[]` |  |

### ModelStatsEvent

Fields the server didn't report are nil.

| Field | Type | Description |
|---|---|---|
| `model` | `string` |  |
| `context_window` | `integer` | In tokens |
| `context_tokens` | `integer?` | The conversation's size after this response |
| `input_tokens` | `integer?` | Prompt tokens the server read now |
| `cached_tokens` | `integer?` | Prompt tokens reused from the server's cache |
| `output_tokens` | `integer?` |  |
| `tok_s` | `number?` | Output tokens per second |
| `prompt_tok_s` | `number?` | Prompt tokens read per second |
| `first_token_ms` | `integer?` | From sending the request to the first token |

### ToolCallEvent

| Field | Type | Description |
|---|---|---|
| `id` | `string` |  |
| `name` | `string` |  |
| `args` | `table` |  |
| `agent` | `string?` | The sub-agent that made the call; nil for the main agent (psi help agents) |

### ToolCallResult

| Field | Type | Description |
|---|---|---|
| `block` | `boolean?` |  |
| `reason` | `string?` | Told to the model when blocked |
| `args` | `table?` | Replacement arguments |

### ToolResultEvent

| Field | Type | Description |
|---|---|---|
| `id` | `string` |  |
| `name` | `string` |  |
| `args` | `table` |  |
| `text` | `string` |  |
| `is_error` | `boolean` |  |
| `agent` | `string?` | The sub-agent that made the call; nil for the main agent (psi help agents) |

### PolicyEvent

| Field | Type | Description |
|---|---|---|
| `tool_call_id` | `string` |  |
| `decision` | `string` |  |
| `reason` | `string` |  |
| `agent` | `string?` | The sub-agent that made the call; nil for the main agent (psi help agents) |

### GuardEvent

| Field | Type | Description |
|---|---|---|
| `guard` | `string` |  |
| `message` | `string` |  |
| `agent` | `string?` | The sub-agent that made the call; nil for the main agent (psi help agents) |

### EventName

The name of an event.

One of: `"session_start"`, `"session_shutdown"`, `"session_before_compact"`, `"input"`, `"before_agent_start"`, `"agent_start"`, `"agent_end"`, `"turn_start"`, `"turn_end"`, `"context"`, `"message_start"`, `"message_update"`, `"message_end"`, `"model_stats"`, `"tool_call"`, `"tool_execution_update"`, `"tool_result"`, `"policy_decision"`, `"guard_event"`.

### ExecOptions

| Field | Type | Description |
|---|---|---|
| `cwd` | `string?` | Where it runs (default: the working directory) |
| `env` | `{string: string}?` | Variables to add |
| `stdin` | `string?` | Its input (default: none) |
| `timeout_ms` | `integer?` | Stopped after this long (default 60000) |
| `max_bytes` | `integer?` | Each of stdout and stderr is cut after this many bytes (default 1048576) |

### ExecResult

| Field | Type | Description |
|---|---|---|
| `code` | `integer?` | The exit code; nil when it was stopped |
| `ok` | `boolean` | It exited with 0 |
| `stdout` | `string` |  |
| `stderr` | `string` |  |
| `killed` | `boolean` | Stopped by the timeout, Esc or a signal |
| `truncated` | `boolean` | stdout or stderr was cut at max_bytes |

### HttpRequest

| Field | Type | Description |
|---|---|---|
| `url` | `string` |  |
| `method` | `string?` | Default GET, or POST with a body |
| `headers` | `{string: string}?` |  |
| `query` | `{string: string}?` | Added to the URL, encoded |
| `body` | `string?` |  |
| `json` | `any` | Sent as a JSON body (and Content-Type) instead of `body` |
| `timeout_ms` | `integer?` | Default 30000 |
| `max_bytes` | `integer?` | The body is cut after this many bytes (default 10485760) |

### HttpResponse

| Field | Type | Description |
|---|---|---|
| `status` | `integer` |  |
| `ok` | `boolean` | The status is 2xx |
| `headers` | `{string: string}` | Lowercase names; repeated headers joined with ", " |
| `body` | `string` |  |
| `truncated` | `boolean` | The body was cut at max_bytes |
| `json` | `function(): any` | The body decoded as JSON (psi.json.decode) |

### JsonOptions

| Field | Type | Description |
|---|---|---|
| `pretty` | `boolean?` | Indented, one value per line |

### Match

A regex match. Positions are byte offsets, 1-based and inclusive, as string.find gives.

| Field | Type | Description |
|---|---|---|
| `text` | `string` | The matched text |
| `start` | `integer` |  |
| `finish` | `integer` |  |
| `groups` | `string[]` | The capture groups in order; "" for one that didn't take part |
| `named` | `{string: string}` | Named groups that took part |

### Stat

| Field | Type | Description |
|---|---|---|
| `kind` | `"file"\|"dir"\|"other"` |  |
| `size` | `integer` | In bytes |
| `modified` | `number` | Seconds since 1970 |

### GlobOptions

| Field | Type | Description |
|---|---|---|
| `cwd` | `string?` | Where to search from |
| `gitignore` | `boolean?` | Skip ignored and hidden files (default true) |
