-- tally: counts the model's tool calls on the current branch. Each call is
-- kept as a session entry, so the count follows /tree and /fork; it shows
-- on the status line and in a widget, and the user can share it with the
-- model or ask the model about it.

local function count(psi)
  local by_tool, total = {}, 0
  for _, e in ipairs(psi.entries("tally")) do
    by_tool[e.tool] = (by_tool[e.tool] or 0) + 1
    total = total + 1
  end
  -- Most used first.
  local names = {}
  for name in pairs(by_tool) do names[#names + 1] = name end
  table.sort(names, function(a, b)
    if by_tool[a] ~= by_tool[b] then return by_tool[a] > by_tool[b] end
    return a < b
  end)
  local parts = {}
  for _, name in ipairs(names) do parts[#parts + 1] = name .. " " .. by_tool[name] end
  return total, parts
end

local function summary(psi)
  local total, parts = count(psi)
  if total == 0 then return nil end
  return total .. " tool calls: " .. table.concat(parts, ", ")
end

local function show(psi)
  local total, parts = count(psi)
  psi.ui.status("tally", total > 0 and (total .. " calls") or nil)
  psi.ui.widget("tally", total > 0 and { psi.style.dim(table.concat(parts, "  ")) } or nil)
end

return {
  name = "tally",
  version = "0.1.0",
  description = "Counts tool calls on the branch",
  setup = function(psi)
    psi.on("tool_result", function(ev)
      psi.append_entry("tally", { tool = ev.name })
      show(psi)
    end)
    psi.on("session_start", function() show(psi) end)
    psi.register_command("tally", {
      description = "Show how many tool calls the model made on this branch",
      handler = function(_, ctx)
        ctx.ui.notify(summary(psi) or "no tool calls yet")
      end,
    })
    psi.register_command("tally-share", {
      description = "Tell the model its tool call count with the next request",
      handler = function()
        local s = summary(psi)
        if s then psi.send_message("tally", "So far: " .. s) else psi.ui.notify("no tool calls yet") end
      end,
    })
    psi.register_command("tally-ask", {
      description = "Ask the model which of its tool calls it could have skipped",
      handler = function()
        local s = summary(psi)
        if not s then return psi.ui.notify("no tool calls yet") end
        psi.send_user_message("You have made " .. s .. ". Which of them could you have skipped, and why?")
      end,
    })
  end,
}
