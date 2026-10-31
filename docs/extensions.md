# Writing extensions

Psi extensions are Lua 5.4 (for hooks, tools, commands and UI) or script
tools (any executable). They are trusted: they run as you, with the full
Lua standard library. Limits are there to catch bugs: 256 MiB of memory,
and code that runs 5 seconds without waiting on psi is stopped (a
`tool_call` handler stopped this way blocks the call).

The full API, generated from `crates/psi-ext-api/api/psi.toml`, is in
[api.md](api.md), with LuaLS stubs in [api/psi.lua](api/psi.lua) for your
editor. `examples/extensions/` has one working, tested example per area.

## Where they load from

- `~/.psi/extensions/`
- `<project>/.psi/extensions/`, once you trust the project: psi asks the
  first time, and again whenever the project's `.psi` files change.
  Headless runs skip an untrusted project's `.psi` (or pass `--trust-project`).
- `--extension <path>`

Each entry is `name.lua`, a directory with `init.lua`, or a script tool
directory with `tool.json`. `/reload` loads them all again.

## A Lua extension

```lua
---@param psi psi
local function setup(psi)
  psi.register_tool{
    name = "line_count",
    description = "Count the lines of a file",
    params = { path = { type = "string", description = "The file" } },
    annotations = { read_only = true },
    execute = function(args, ctx)
      local r = ctx.execute_tool("read", { path = args.path })
      if r.is_error then return r end
      local n = select(2, r.text:gsub("\n", "")) + 1
      return args.path .. " has " .. n .. " lines"
    end,
  }
  psi.on("tool_call", function(ev, ctx)
    if ev.name == "bash" and ev.args.command:find("rm -rf /", 1, true) then
      return { block = true, reason = "not that" }
    end
  end)
end

return { name = "my-ext", version = "0.1.0", description = "What it does", setup = setup }
```

Event kinds: `notify` handlers' results are ignored; `chain` handlers may
return a table that changes the payload for the next handler; `gate`
handlers may block (`{ block = true, reason = ... }`) or rewrite
(`{ args = {...} }`). A rewrite happens before the policy checks the call,
and is recorded in the session.

Tools from extensions go through the same policy as built-in ones:
`always_ask = true` makes every call ask; in strict mode, a tool that isn't
`read_only` asks.

## A script tool

A directory with `tool.json` and an executable. The arguments arrive as
JSON on stdin; what it prints is the result; a non-zero exit marks it
failed. It runs inside the same sandbox as bash.

```json
{
  "name": "word_count",
  "description": "Count the words in a file",
  "params": { "path": { "type": "string", "description": "The file" } },
  "read_only": true,
  "command": "run.py"
}
```

## Checking and testing

- `psi ext check <path>`: the manifest, syntax, API use (unknown members,
  argument counts and types, unknown events), accidental globals, a dry
  load, and lua-language-server when it is installed.
- `psi ext test <path>`: runs `*_test.lua` beside the extension against a
  fake host, and shows everything the extension printed, logged or threw.

```lua
test.case("blocks rm -rf /", function()
  local r = test.emit("tool_call", { id = "1", name = "bash", args = { command = "rm -rf /" } })
  test.truthy(r.blocked)
end)
```

Helpers: `test.case(name, fn)`, `test.emit(event, payload)` (returns
`{ payload, results, blocked }`), `test.tool(name, args)`,
`test.command(name, args)`, `test.shortcut(key)`,
`test.render(tool, call, width)`, `test.ui.answer(kind, value)` for
confirm, select, input and custom, `test.mock_tool(name, result)` for
`execute_tool`, `test.requests(kind)` (everything the extension asked of
psi), `test.clear()`, `test.set_entries({{type, data}, ...})`,
`test.eq`, `test.truthy`, `test.contains`, `test.clock.advance(seconds)`
and `test.tmp` (the test's working directory). Each case starts clean.
