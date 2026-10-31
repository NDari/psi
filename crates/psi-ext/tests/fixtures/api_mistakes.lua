return {
  name = "api-mistakes",
  version = "0.1.0",
  description = "Every API mistake the static pass catches",
  setup = function(psi)
    psi.ui.notfy("hi")
    psi.ui.confirm("Sure?")
    psi.ui.notify(42)
    psi.ui.status({}, "x")
    psi.on("tool_cal", function() end)
    psi.ui.notify("x", "warn")
    psi.ui.notify("a", "info", "extra")
    psi.on("tool_call", function(ev, ctx)
      ctx.ui.selct("Pick", { "a" })
      ctx.execute_tool("read")
      return { block = ctx.cdw == ev.name }
    end)
    local ui = psi.ui
    ui.notify(nil)
    psi:log("x")
    psi.set_active_tools({ "read", 2 })
    psi.get_active_tools(1)
    psi.registr_tool({})
    local n = select("#", 1)
    psi.ui.confirm(table.unpack({ "a", tostring(n) }))
    psi.register_tool({ name = "t", descripton = "x", params = {}, execute = function() end })
    psi.send_user_message("go", { deliver = "follow_up" })
  end,
}
