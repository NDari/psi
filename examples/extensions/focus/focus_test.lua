local ALL = { "read", "write", "edit", "bash", "grep" }

test.case("the flag has its default", function()
  test.eq(psi.get_flag("focus"), "off")
end)

test.case("ctrl+r keeps only reading tools, then brings the rest back", function()
  psi.set_active_tools(ALL)
  test.clear()
  test.shortcut("ctrl+r")
  test.eq(test.requests("set_active_tools")[1].names, { "read", "grep" })
  test.eq(test.requests("status")[1].text, "read-only")
  test.eq(test.requests("emit")[1], { kind = "emit", name = "focus", data = { on = true } })
  test.clear()
  test.shortcut("ctrl+r")
  test.eq(test.requests("set_active_tools")[1].names, ALL)
  test.eq(test.requests("status")[1].text, nil)
end)

test.case("with the flag off, a new session changes nothing", function()
  test.emit("session_start", { session_file = "s.jsonl" })
  test.eq(#test.requests("set_active_tools"), 0)
end)
