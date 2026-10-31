return {
  name = "luals-payload",
  version = "0.1.0",
  description = "A typo in an event payload field",
  setup = function(psi)
    psi.on("tool_call", function(ev, ctx)
      if ev.nme == "bash" then ctx.ui.notify("bash") end
    end)
    psi.ui.notfy("x")
  end,
}
