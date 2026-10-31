-- focus: a flag, a shortcut and the event bus. Ctrl+R switches the model to
-- read-only tools and back (set_active_tools); `--ext-flag focus=on` starts
-- in read-only mode; other extensions hear about it on the bus.

local READ_ONLY = { read = true, grep = true, find = true, ls = true }

return {
  name = "focus",
  version = "0.1.0",
  description = "Read-only mode on a shortcut",
  setup = function(psi)
    psi.register_flag("focus", { description = "on: start in read-only mode", default = "off" })
    local all = nil
    local function set(on)
      all = all or psi.get_active_tools()
      if on then
        local keep = {}
        for _, t in ipairs(all) do if READ_ONLY[t] then keep[#keep + 1] = t end end
        psi.set_active_tools(keep)
      else
        psi.set_active_tools(all)
      end
      psi.ui.status("focus", on and "read-only" or nil)
      psi.events.emit("focus", { on = on })
    end
    local on = psi.get_flag("focus") == "on"
    psi.register_shortcut("ctrl+r", {
      description = "switch read-only mode",
      handler = function() on = not on; set(on) end,
    })
    psi.on("session_start", function() if on then set(true) end end)
    psi.events.on("focus", function(data) psi.log("focus is " .. (data.on and "on" or "off")) end)
  end,
}
