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

Send a message as the user. While the agent runs it steers the run, or waits for the run to end with `deliver = "follow_up"`; otherwise it starts a run.

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

One of: `"session_start"`, `"session_shutdown"`, `"session_before_compact"`, `"input"`, `"before_agent_start"`, `"agent_start"`, `"agent_end"`, `"turn_start"`, `"turn_end"`, `"context"`, `"message_start"`, `"message_update"`, `"message_end"`, `"tool_call"`, `"tool_execution_update"`, `"tool_result"`, `"policy_decision"`, `"guard_event"`.
