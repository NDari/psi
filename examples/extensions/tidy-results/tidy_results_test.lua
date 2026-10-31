local function bash_result(text)
  return test.emit("tool_result", { id = "c1", name = "bash", args = { command = "pytest" }, text = text, is_error = false })
end

test.case("long bash output keeps its start and end", function()
  local lines = {}
  for i = 1, 200 do lines[#lines + 1] = "line " .. i end
  local r = bash_result(table.concat(lines, "\n"))
  test.contains(r.payload.text, "line 1\n")
  test.contains(r.payload.text, "[tidy-results: 150 lines left out]")
  test.contains(r.payload.text, "line 200")
end)

test.case("short output and other tools are left alone", function()
  test.eq(bash_result("3 passed").payload.text, "3 passed")
  local r = test.emit("tool_result", { id = "c2", name = "read", args = {}, text = string.rep("x\n", 100), is_error = false })
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
