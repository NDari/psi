local function bash_result(text, is_error)
  return test.emit("tool_result", { id = "c1", name = "bash", args = { command = "pytest" }, text = text, is_error = is_error or false })
end

test.case("color codes are removed", function()
  local r = bash_result("\27[32m3 passed\27[0m in \27[1;33m0.2s\27[0m")
  test.eq(r.payload.text, "3 passed in 0.2s")
end)

test.case("a progress bar keeps only its last state", function()
  local r = bash_result("downloading\n 10%\r 55%\r100%\ndone", true)
  test.eq(r.payload.text, "downloading\n100%\ndone")
  test.eq(r.payload.is_error, true)
end)

test.case("plain output and other tools are left alone", function()
  test.eq(#bash_result("3 passed").results, 0)
  local r = test.emit("tool_result", { id = "c2", name = "read", args = {}, text = "\27[31mx\27[0m", is_error = false })
  test.eq(#r.results, 0)
end)

test.case("renders bash calls on one line", function()
  local lines = test.render("bash", { name = "bash", args = { command = "cargo test" }, result = "", is_error = false }, 40)
  test.eq(#lines, 1)
  test.contains(lines[1], "$ cargo test")
end)

test.case("adds a system line", function()
  test.eq(test.emit("before_agent_start", { prompt = "x", system = "" }).payload.system, "Run the tests after every change.")
end)
