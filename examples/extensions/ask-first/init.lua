-- ask-first: the UI primitives. A /deploy command that confirms, picks a
-- target, asks for a note, and shows a small custom component; and a
-- `deploy` tool that always asks before it runs.

local function picker(title, options)
  -- A custom component (pi-tui parity): render(width) gives the lines,
  -- handle_input(key) moves or answers; a non-nil answer closes it.
  local sel = 1
  return {
    render = function(width)
      local lines = { psi.style.bold(title) }
      for i, o in ipairs(options) do
        local line = (i == sel and "> " or "  ") .. o
        lines[#lines + 1] = i == sel and psi.style.color("cyan", line) or line
      end
      return lines
    end,
    handle_input = function(key)
      if key == "up" then sel = math.max(1, sel - 1)
      elseif key == "down" then sel = math.min(#options, sel + 1)
      elseif key == "enter" then return options[sel]
      elseif key == "escape" then return false
      end
      return nil
    end,
  }
end

return {
  name = "ask-first",
  version = "0.1.0",
  description = "Confirm, select, input and a custom component",
  setup = function(psi)
    psi.register_command("deploy", {
      description = "Deploy, after asking",
      handler = function(args, ctx)
        if not ctx.has_ui then return ctx.ui.notify("deploy needs a person to answer", "warning") end
        if not ctx.ui.confirm("Deploy?", "This deploys the current branch.") then
          return ctx.ui.notify("not deployed")
        end
        local target = ctx.ui.select("Where to?", { "staging", "production" })
        if not target then return ctx.ui.notify("no target chosen") end
        local note = ctx.ui.input("A note for the log", "optional") or ""
        local speed = ctx.ui.custom(picker("How fast?", { "canary", "all at once" }))
        if not speed then return ctx.ui.notify("cancelled") end
        psi.append_entry("deploy", { target = target, note = note, speed = speed })
        ctx.ui.notify("deploying to " .. target .. " (" .. speed .. ")")
      end,
    })
    psi.register_tool{
      name = "deploy",
      description = "Deploy the current branch to staging",
      params = {},
      always_ask = true,
      execute = function(_, ctx)
        local r = ctx.execute_tool("bash", { command = "echo deployed" })
        return r.text
      end,
    }
  end,
}
