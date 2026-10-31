local M = {}
print("loaded good")

local function count(text)
  local n = 0
  for _ in text:gmatch("%S+") do n = n + 1 end
  return n
end

return {
  name = "good",
  version = "0.1.0",
  description = "Exercises the whole API",
  setup = function(psi)
    psi.register_tool({
      name = "word_count",
      description = "Count the words in a file",
      params = { path = { type = "string", description = "The file" } },
      required = { "path" },
      annotations = { read_only = true },
      execute = function(args, ctx)
        local f = assert(io.open(ctx.cwd .. "/" .. args.path))
        local n = count(f:read("a"))
        f:close()
        return { text = tostring(n) }
      end,
    })
    psi.register_command("todo", {
      description = "Show the todo list",
      handler = function(args, ctx)
        ctx.ui.notify("todo: " .. args, "info")
      end,
    })
    psi.register_shortcut("ctrl+t", { description = "Todo", handler = function(ctx) ctx.ui.status("todo", nil) end })
    psi.register_flag("verbose", { description = "Say more", default = false })
    psi.on("tool_call", function(ev, ctx)
      if ev.name == "bash" and ev.args.command:find("rm %-rf /") then
        if not ctx.ui.confirm("Really?", ev.args.command) then
          return { block = true, reason = "the user said no" }
        end
      end
    end)
    psi.on("session_start", function(_, ctx) psi.ui.widget("todo", { ctx.cwd }) end)
    psi.events.on("todo_changed", function(data) M.last = data end)
    psi.register_tool_renderer("word_count", function(call, width)
      return { psi.style.bold(call.name):sub(1, width) }
    end)
    psi.log("setup done")
  end,
}
