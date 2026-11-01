-- tidy-results: tool_result (chain) strips terminal noise (color codes,
-- progress bars redrawn with \r) from bash output before the model sees
-- it, a tool renderer draws bash calls its own way, and
-- before_agent_start adds a line to the system prompt.

local function tidy(text)
  local out, changed = {}, false
  for line in (text .. "\n"):gmatch("(.-)\n") do
    -- A terminal shows only what follows the last carriage return.
    local shown = line:match("([^\r]*)\r*$") or line
    shown = shown:gsub("\27%[[%d;?]*[A-Za-z]", "")
    if shown ~= line then changed = true end
    out[#out + 1] = shown
  end
  if not changed then return nil end
  return table.concat(out, "\n")
end

return {
  name = "tidy-results",
  version = "0.1.0",
  description = "Plain bash output, a bash renderer, a system line",
  setup = function(psi)
    psi.on("tool_result", function(ev)
      if ev.name ~= "bash" then return nil end
      local clean = tidy(ev.text)
      if clean then return { text = clean, is_error = ev.is_error } end
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
