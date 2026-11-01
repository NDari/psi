local KEEP = 10

local function split(text)
  local lines = {}
  for line in (text .. "\n"):gmatch("(.-)\n") do lines[#lines + 1] = line end
  return lines
end

return {
  name = "trim_output",
  version = "0.1.0",
  description = "Trims long bash output",
  setup = function(psi)
    psi.register_flag("max_lines", { description = "Bash results longer than this are trimmed", default = 40 })
    psi.on("tool_result", function(ev)
      if ev.name ~= "bash" then return end
      local max = tonumber(psi.get_flag("max_lines")) or 40
      local lines = split(ev.text)
      if #lines <= max then return end
      local out = {}
      for i = 1, KEEP do out[#out + 1] = lines[i] end
      out[#out + 1] = string.format("[... %d lines trimmed ...]", #lines - 2 * KEEP)
      for i = #lines - KEEP + 1, #lines do out[#out + 1] = lines[i] end
      return { text = table.concat(out, "\n"), is_error = ev.is_error }
    end)
  end,
}
