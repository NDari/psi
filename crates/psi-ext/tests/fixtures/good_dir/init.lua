local lint = require("lint")

return {
  name = "good-dir",
  version = "1.0.0",
  description = "A directory extension with a module",
  setup = function(p)
    p.register_tool {
      name = "lint_tool",
      description = "Lint a file",
      params = { file = { type = "string", description = "The file" } },
      execute = function(args) return lint.run(args.file) end,
    }
    p.on("tool_result", function(ev, ctx)
      if ev.is_error then ctx.ui.notify(ev.name .. " failed", "warning") end
    end)
  end,
}
