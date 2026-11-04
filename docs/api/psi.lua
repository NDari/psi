---@meta
-- LuaLS stubs for the psi extension API. Generated from crates/psi-ext-api/api
-- by build.rs; do not edit. Type setup's argument with: ---@param psi psi

--- The name of an event.
---@alias EventName
---| "session_start" # The session is open (also after /new, /resume and /reload).
---| "session_shutdown" # psi is closing the session.
---| "session_before_compact" # Old turns are about to be summarized or dropped to fit the context window. Return { cancel = true } to stop it.
---| "input" # The user submitted text that is not a command (extension commands run first). Return { text = ... } to change it, or { handled = true } to consume it; skills and prompt templates expand after.
---| "before_agent_start" # A run is about to start. Return { system = ... } to add text to the end of the system prompt for this run.
---| "agent_start" # A run started.
---| "agent_end" # A run ended.
---| "turn_start" # A model request is about to be sent.
---| "turn_end" # A turn (the request and its tool calls) finished.
---| "context" # The messages about to be sent to the model (read-only).
---| "message_start" # The model started answering.
---| "message_update" # Streamed answer text.
---| "message_end" # The model's answer is complete.
---| "model_stats" # A model response finished: its tokens and speed, for a status line segment (psi.ui.status).
---| "tool_call" # A tool call is about to be checked and run. Return { block = true, reason = ... } to refuse it, or { args = {...} } to rewrite its arguments (the rewrite is checked by the policy and recorded).
---| "tool_execution_update" # A tool call is running.
---| "tool_result" # A tool call finished. Return { text = ..., is_error = ... } to change what the model sees.
---| "policy_decision" # The policy asked, refused, or the user answered.
---| "guard_event" # A guard fired (a loop, an error streak, a failing checkpoint).

--- The second argument of every handler.
---@class Context
---@field cwd string The working directory
---@field mode "interactive"|"print"|"json"|"test" How psi runs
---@field has_ui boolean Whether a person can answer ui calls
---@field session_file? string The session file
---@field ui table psi.ui
---@field execute_tool fun(name: string, args: table): ToolResult psi.execute_tool

---@class ToolSpec
---@field name string Lowercase, with _ between words
---@field description string What it does and when to use it, for the model
---@field params table<string, ParamSpec> The arguments
---@field required? string[] Required argument names (default: all)
---@field annotations? ToolAnnotations
---@field always_ask? boolean Every call asks the user
---@field execute fun(args: table, ctx: Context): string|ToolResult Runs a call; an error becomes a failed result

---@class ParamSpec
---@field type "string"|"integer"|"number"|"boolean"|"array"|"object"
---@field description string

---@class ToolAnnotations
---@field read_only? boolean Only reads: may run in parallel, and no checkpoint follows
---@field destructive? boolean
---@field open_world? boolean Reaches outside the workspace (network, other systems)

---@class ToolResult
---@field text string
---@field is_error? boolean

---@class CommandSpec
---@field description string Shown in /help and completion
---@field handler fun(args: string, ctx: Context)

---@class ProviderSpec
---@field base_url string The OpenAI-compatible API, like http://127.0.0.1:8080/v1
---@field api_key? string Sent as a bearer token; "$NAME" reads the environment variable NAME
---@field headers? table<string, string> More request headers; values take "$NAME" too
---@field models ProviderModel[] The models it serves

---@class ProviderModel
---@field id string The model name sent in requests
---@field context_window? integer Tokens; default: what the server's /props says
---@field max_output? integer Tokens per answer (default 8192)
---@field enable_thinking? boolean Sent as chat_template_kwargs.enable_thinking

---@class ShortcutSpec
---@field description string
---@field handler fun(ctx: Context)

---@class FlagSpec
---@field description string
---@field default any

---@class SendOptions
---@field deliver? "steer"|"follow_up" While a run is going (default steer)

---@class ToolRenderCall
---@field name string
---@field args table
---@field result string
---@field is_error boolean

--- A custom UI component (pi-tui parity), drawn under the input box under a deadline.
---@class Component
---@field render fun(width: integer): string[] The lines to show
---@field handle_input fun(key: string): any A key like "enter", "up", "a"; a non-nil return closes it
---@field dispose? fun()

---@class SessionEvent
---@field session_file? string

---@class CompactEvent
---@field tokens integer The context size, in tokens
---@field reason? string "summary" (past the trigger) or "overflow" (the server refused the request)

---@class GateResult
---@field cancel? boolean

---@class InputEvent
---@field text string As typed: before /skill: and prompt template expansion
---@field source "interactive"|"print"|"extension" Typed in the TUI, the headless prompt (-p or --json), or psi.send_user_message

---@class InputResult
---@field text? string
---@field handled? boolean

---@class AgentStartEvent
---@field prompt string

---@class AgentStartResult
---@field system? string Appended to the system prompt for this run

---@class AgentEndEvent
---@field stop string done, budget, guard, aborted or failed
---@field text string The last answer

---@class TurnEvent
---@field turn integer

---@class ContextEvent
---@field messages table[] In OpenAI chat format

---@class MessageUpdateEvent
---@field text string The new text

---@class MessageEndEvent
---@field text string
---@field tool_calls table[]

--- Fields the server didn't report are nil.
---@class ModelStatsEvent
---@field model string
---@field context_window integer In tokens
---@field context_tokens? integer The conversation's size after this response
---@field input_tokens? integer Prompt tokens the server read now
---@field cached_tokens? integer Prompt tokens reused from the server's cache
---@field output_tokens? integer
---@field tok_s? number Output tokens per second
---@field prompt_tok_s? number Prompt tokens read per second
---@field first_token_ms? integer From sending the request to the first token

---@class ToolCallEvent
---@field id string
---@field name string
---@field args table
---@field agent? string The sub-agent that made the call; nil for the main agent (psi help agents)

---@class ToolCallResult
---@field block? boolean
---@field reason? string Told to the model when blocked
---@field args? table Replacement arguments

---@class ToolResultEvent
---@field id string
---@field name string
---@field args table
---@field text string
---@field is_error boolean
---@field agent? string The sub-agent that made the call; nil for the main agent (psi help agents)

---@class PolicyEvent
---@field tool_call_id string
---@field decision string
---@field reason string
---@field agent? string The sub-agent that made the call; nil for the main agent (psi help agents)

---@class GuardEvent
---@field guard string
---@field message string
---@field agent? string The sub-agent that made the call; nil for the main agent (psi help agents)

---@class ExecOptions
---@field cwd? string Where it runs (default: the working directory)
---@field env? table<string, string> Variables to add
---@field stdin? string Its input (default: none)
---@field timeout_ms? integer Stopped after this long (default 60000)
---@field max_bytes? integer Each of stdout and stderr is cut after this many bytes (default 1048576)

---@class ExecResult
---@field code? integer The exit code; nil when it was stopped
---@field ok boolean It exited with 0
---@field stdout string
---@field stderr string
---@field killed boolean Stopped by the timeout, Esc or a signal
---@field truncated boolean stdout or stderr was cut at max_bytes

---@class HttpRequest
---@field url string
---@field method? string Default GET, or POST with a body
---@field headers? table<string, string>
---@field query? table<string, string> Added to the URL, encoded
---@field body? string
---@field json any Sent as a JSON body (and Content-Type) instead of `body`
---@field timeout_ms? integer Default 30000
---@field max_bytes? integer The body is cut after this many bytes (default 10485760)

---@class HttpResponse
---@field status integer
---@field ok boolean The status is 2xx
---@field headers table<string, string> Lowercase names; repeated headers joined with ", "
---@field body string
---@field truncated boolean The body was cut at max_bytes
---@field json fun(): any The body decoded as JSON (psi.json.decode)

---@class JsonOptions
---@field pretty? boolean Indented, one value per line

--- A regex match. Positions are byte offsets, 1-based and inclusive, as string.find gives.
---@class Match
---@field text string The matched text
---@field start integer
---@field finish integer
---@field groups string[] The capture groups in order; "" for one that didn't take part
---@field named table<string, string> Named groups that took part

---@class Stat
---@field kind "file"|"dir"|"other"
---@field size integer In bytes
---@field modified number Seconds since 1970

---@class GlobOptions
---@field cwd? string Where to search from
---@field gitignore? boolean Skip ignored and hidden files (default true)

---@class psi
local psi = {}

---@class psi.events
psi.events = {}

---@class psi.ui
psi.ui = {}

---@class psi.style
psi.style = {}

---@class psi.bash
psi.bash = {}

---@class psi.http
psi.http = {}

---@class psi.json
psi.json = {}

---@class psi.re
psi.re = {}

---@class psi.path
psi.path = {}

---@class psi.fs
psi.fs = {}

---@class psi.toml
psi.toml = {}

---@class psi.base64
psi.base64 = {}

---@class psi.hash
psi.hash = {}

---@class psi.time
psi.time = {}

--- Subscribe to an event. Gate and chain events use the handler's return value (see the event); errors are reported and never crash psi, and a failing `tool_call` handler blocks the call.
---@param event EventName
---@param handler fun(ev: table, ctx: Context): any
---@overload fun(event: "session_start", handler: fun(ev: SessionEvent, ctx: Context))
---@overload fun(event: "session_shutdown", handler: fun(ev: SessionEvent, ctx: Context))
---@overload fun(event: "session_before_compact", handler: fun(ev: CompactEvent, ctx: Context): GateResult?)
---@overload fun(event: "input", handler: fun(ev: InputEvent, ctx: Context): InputResult?)
---@overload fun(event: "before_agent_start", handler: fun(ev: AgentStartEvent, ctx: Context): AgentStartResult?)
---@overload fun(event: "agent_start", handler: fun(ev: AgentStartEvent, ctx: Context))
---@overload fun(event: "agent_end", handler: fun(ev: AgentEndEvent, ctx: Context))
---@overload fun(event: "turn_start", handler: fun(ev: TurnEvent, ctx: Context))
---@overload fun(event: "turn_end", handler: fun(ev: TurnEvent, ctx: Context))
---@overload fun(event: "context", handler: fun(ev: ContextEvent, ctx: Context))
---@overload fun(event: "message_start", handler: fun(ev: TurnEvent, ctx: Context))
---@overload fun(event: "message_update", handler: fun(ev: MessageUpdateEvent, ctx: Context))
---@overload fun(event: "message_end", handler: fun(ev: MessageEndEvent, ctx: Context))
---@overload fun(event: "model_stats", handler: fun(ev: ModelStatsEvent, ctx: Context))
---@overload fun(event: "tool_call", handler: fun(ev: ToolCallEvent, ctx: Context): ToolCallResult?)
---@overload fun(event: "tool_execution_update", handler: fun(ev: ToolCallEvent, ctx: Context))
---@overload fun(event: "tool_result", handler: fun(ev: ToolResultEvent, ctx: Context): ToolResult?)
---@overload fun(event: "policy_decision", handler: fun(ev: PolicyEvent, ctx: Context))
---@overload fun(event: "guard_event", handler: fun(ev: GuardEvent, ctx: Context))
function psi.on(event, handler) end

--- Add a tool the model can call. It goes through the same policy as built-in tools: `always_ask` makes every call ask.
---@param spec ToolSpec
function psi.register_tool(spec) end

--- Add a slash command (`/name args`) to the interactive mode.
---@param name string
---@param spec CommandSpec
function psi.register_command(name, spec) end

--- Bind a key (like "ctrl+t") in the interactive mode.
---@param key string
---@param spec ShortcutSpec
function psi.register_shortcut(key, spec) end

--- Declare a flag set on the command line with `--ext-flag name=value`.
---@param name string
---@param spec FlagSpec
function psi.register_flag(name, spec) end

--- Add an OpenAI-compatible endpoint and its models; pick one with `psi --model name/id` (or the id alone). Registering a name again replaces it.
---@param name string
---@param spec ProviderSpec
function psi.register_provider(name, spec) end

--- A flag's value: the command line's, else its default.
---@param name string
---@return any
function psi.get_flag(name) end

--- Draw a tool's committed call yourself: return the lines (style them with psi.style).
---@param tool string
---@param render fun(call: ToolRenderCall, width: integer): string[]
function psi.register_tool_renderer(tool, render) end

--- Send a message as the user. While the agent runs it steers the run, or waits for the run to end with `deliver = "follow_up"`; otherwise it starts a run.
---@param text string
---@param opts? SendOptions
function psi.send_user_message(text, opts) end

--- Add a message from the extension that the model sees with the next request (a `custom_message` entry).
---@param custom_type string
---@param text string
function psi.send_message(custom_type, text) end

--- Store data in the session (a `custom` entry, never sent to the model).
---@param custom_type string
---@param data any
function psi.append_entry(custom_type, data) end

--- The current branch's entries of one custom type, oldest first, for state kept with append_entry.
---@param custom_type string
---@return table[]
function psi.entries(custom_type) end

--- Choose which tools the model is offered from the next request on.
---@param names string[]
function psi.set_active_tools(names) end

--- The names of the tools the model is offered.
---@return string[]
function psi.get_active_tools() end

--- Run a tool as the model would, through the policy (it may ask the user). Also `ctx.execute_tool`.
---@async
---@param name string
---@param args table
---@return ToolResult
function psi.execute_tool(name, args) end

--- Send an event to every extension's events.on handlers for that name.
---@param name string
---@param data any
function psi.events.emit(name, data) end

--- Listen to events.emit calls from any extension.
---@param name string
---@param handler fun(data: any)
function psi.events.on(name, handler) end

--- Show a one-line message. In headless runs it goes to the log.
---@param message string
---@param level? "info"|"warning"|"error"
function psi.ui.notify(message, level) end

--- Ask the user a yes/no question. Returns false in headless modes.
---@async
---@param title string
---@param message string
---@return boolean
function psi.ui.confirm(title, message) end

--- Let the user pick one option. Returns nil if they cancel, and in headless modes.
---@async
---@param title string
---@param options string[]
---@return string?
function psi.ui.select(title, options) end

--- Ask the user for a line of text. Returns nil if they cancel, and in headless modes.
---@async
---@param title string
---@param placeholder? string
---@return string?
function psi.ui.input(title, placeholder) end

--- Set (or clear, with nil) a short item on the status line.
---@param key string
---@param text? string
function psi.ui.status(key, text) end

--- Show lines above the input box under a slot name (nil removes them).
---@param slot string
---@param lines? string[]
function psi.ui.widget(slot, lines) end

--- Show a component under the input box until its handle_input returns a value, which ui.custom returns. Returns nil in headless modes.
---@async
---@param component Component
---@return any
function psi.ui.custom(component) end

--- Text in one of the terminal's 16 colors: black, red, green, yellow, blue, magenta, cyan, white, and bright_<color>.
---@param color string
---@param text string
---@return string
function psi.style.color(color, text) end

--- Bold text.
---@param text string
---@return string
function psi.style.bold(text) end

--- Dimmed text.
---@param text string
---@return string
function psi.style.dim(text) end

--- Write to psi's log (PSI_LOG=info shows it); `psi ext test` prints it.
---@param message string
function psi.log(message) end

--- Run a program and wait for it, inside the same boundary as bash, with bash's environment. Commands on the always-ask list ask the user (the card names this extension); headless runs refuse them. `{"sh", "-c", script}` is checked like a bash command. Errors when refused, denied or the program can't start; a non-zero exit is a result. Esc stops it. Use it instead of io.popen and os.execute, which extensions don't have.
---@async
---@param argv string[]
---@param opts? ExecOptions
---@return ExecResult
function psi.exec(argv, opts) end

--- Why a bash command may change something, or nil when it only reads: every program in it (through pipes, `&&`, substitutions and `sh -c`) is a known reader such as `cat`, `grep` or `git diff`, and nothing is redirected into a file. Plan mode uses the same check. A `tool_call` gate can block writes to a file and let reads of it through: `if cmd:find("docs/words.md", 1, true) and psi.bash.writes(cmd) then ... end`.
---@param command string
---@return string?
function psi.bash.writes(command) end

--- Make an HTTP or HTTPS request. Errors when the network is off (the `network` setting), on a bad URL, or when the connection fails or times out; any status is a response. Esc stops it.
---@async
---@param req HttpRequest
---@return HttpResponse
function psi.http.request(req) end

--- A GET request: psi.http.request{ url = url }.
---@async
---@param url string
---@return HttpResponse
function psi.http.get(url) end

--- A value as JSON. Keys come out sorted. An empty table is {} unless marked with psi.json.array; functions are an error.
---@param value any
---@param opts? JsonOptions
---@return string
function psi.json.encode(value, opts) end

--- Parses JSON. null becomes psi.json.null() (so arrays keep their length), and arrays decode marked as arrays.
---@param text string
---@return any
function psi.json.decode(text) end

--- The value JSON null decodes to; put it in a table to encode a null.
---@return any
function psi.json.null() end

--- Marks a table as an array, so it encodes as [] even when empty. Returns the table.
---@param t table
---@return table
function psi.json.array(t) end

--- Whether `pattern` matches anywhere in `text`. Patterns use Rust regex syntax (Perl-like, without lookaround or backreferences); (?i) ignores case.
---@param pattern string
---@param text string
---@return boolean
function psi.re.is_match(pattern, text) end

--- The first match, or nil.
---@param pattern string
---@param text string
---@return Match?
function psi.re.find(pattern, text) end

--- Every match that doesn't overlap the one before, in order.
---@param pattern string
---@param text string
---@return Match[]
function psi.re.find_all(pattern, text) end

--- `text` with matches replaced: all of them, or the first `limit`. In `replacement`, $1 and ${name} are capture groups and $$ is a dollar sign.
---@param pattern string
---@param text string
---@param replacement string
---@param limit? integer
---@return string
function psi.re.replace(pattern, text, replacement, limit) end

--- `text` split at each match.
---@param pattern string
---@param text string
---@return string[]
function psi.re.split(pattern, text) end

--- `s` with every regex special character escaped, so a pattern matches it literally: `"^" .. psi.re.escape(dir)`.
---@param s string
---@return string
function psi.re.escape(s) end

--- `part` under `base`; an absolute `part` replaces `base`.
---@param base string
---@param part string
---@return string
function psi.path.join(base, part) end

--- The path with `.` and `..` resolved, without looking at the disk.
---@param p string
---@return string
function psi.path.normalize(p) end

--- The path from the working directory, normalized.
---@param p string
---@return string
function psi.path.absolute(p) end

--- `p` relative to `base` (default: the working directory), with `..` where needed; a result starting with `..` lies outside `base`.
---@param p string
---@param base? string
---@return string
function psi.path.relative(p, base) end

--- The last part: "c.txt" for "a/b/c.txt".
---@param p string
---@return string
function psi.path.basename(p) end

--- Everything but the last part: "a/b" for "a/b/c.txt", "." for "c.txt".
---@param p string
---@return string
function psi.path.dirname(p) end

--- The extension without the dot: "txt" for "a/b/c.txt", "" when there is none.
---@param p string
---@return string
function psi.path.ext(p) end

--- A file's contents. Errors when it can't be read.
---@param path string
---@return string
function psi.fs.read(path) end

--- Writes a file, creating its directories. Inside the workspace, the next checkpoint captures the change.
---@param path string
---@param data string
function psi.fs.write(path, data) end

--- Whether the path exists.
---@param path string
---@return boolean
function psi.fs.exists(path) end

--- What the path is (following links), or nil when it doesn't exist.
---@param path string
---@return Stat?
function psi.fs.stat(path) end

--- The names in a directory, sorted.
---@param dir string
---@return string[]
function psi.fs.list(dir) end

--- Files matching a glob like "src/**/*.rs" (`*` stays within one directory), relative to the working directory or `opts.cwd`, sorted. Skips what .gitignore ignores, hidden files and .git, unless `gitignore = false`.
---@param pattern string
---@param opts? GlobOptions
---@return string[]
function psi.fs.glob(pattern, opts) end

--- Parses a TOML document into a table; dates become strings.
---@param text string
---@return table
function psi.toml.decode(text) end

--- A table as a TOML document.
---@param t table
---@return string
function psi.toml.encode(t) end

--- Standard base64, with padding.
---@param data string
---@return string
function psi.base64.encode(data) end

--- Decodes standard base64. Errors on anything else.
---@param text string
---@return string
function psi.base64.decode(text) end

--- The SHA-256 of the data, in lowercase hex.
---@param data string
---@return string
function psi.hash.sha256(data) end

--- Seconds since 1970 (UTC), with a fraction.
---@return number
function psi.time.now() end

--- A time in local time, with strftime codes (default "%Y-%m-%dT%H:%M:%S%:z"); `t` defaults to now.
---@param t? number
---@param fmt? string
---@return string
function psi.time.format(t, fmt) end

--- Seconds since 1970 for a time written as RFC 3339 ("2026-11-02T10:00:00Z"), or with strftime codes in `fmt` (local time unless it has %z). nil when it doesn't parse.
---@param text string
---@param fmt? string
---@return number?
function psi.time.parse(text, fmt) end
