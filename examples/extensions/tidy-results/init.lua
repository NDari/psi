-- tidy-results: tool_result (chain) keeps long test runs short for the
-- model, a tool renderer draws bash calls its own way, and
-- before_agent_start adds a line to the system prompt.

local MAX_LINES = 60

local function tidy(text)
  local lines = {}
  for line in (text .. "\n"):gmatch("(.-)\n") do lines[#lines + 1] = line end
  if #lines <= MAX_LINES then return nil end
  -- Keep the start and the end, where test runners put the summary.
  local head, tail = {}, {}
  for i = 1, 10 do head[#head + 1] = lines[i] end
  for i = #lines - 39, #lines do tail[#tail + 1] = lines[i] end
  return table.concat(head, "\n") .. "\n[tidy-results: " .. (#lines - 50) .. " lines left out]\n" .. table.concat(tail, "\n")
end

return {
  name = "tidy-results",
  version = "0.1.0",
  description = "Shorter test output, a bash renderer, a system line",
  setup = function(psi)
    psi.on("tool_result", function(ev)
      if ev.name ~= "bash" then return nil end
      local short = tidy(ev.text)
      if short then return { text = short } end
      return nil
    end)
    psi.register_tool_renderer("bash", function(call, width)
      local mark = call.is_error and psi.style.color("red", "x") or psi.style.color("green", "ok")
      local cmd = call.args.command or ""
      if #cmd > width - 10 then cmd = cmd:sub(1, width - 13) .. "..." end
      return { mark .. " $ " .. cmd }
    end)
    psi.on("before_agent_start", function()
      return { system = "Run the tests after every change." }
    end)
  end,
}
