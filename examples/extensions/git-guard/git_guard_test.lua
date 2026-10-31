local function call(command)
  return test.emit("tool_call", { id = "c1", name = "bash", args = { command = command } })
end

test.case("blocks a force push to main", function()
  local r = call("git push --force origin main")
  test.truthy(r.blocked)
  test.contains(r.payload.reason, "force-pushing main is not allowed")
  test.eq(test.requests("log")[1].message, "blocked a force push to main")
end)

test.case("blocks -f and +refspec forms", function()
  test.truthy(call("git push -f origin master").blocked)
  test.truthy(call("git push origin +main").blocked)
end)

test.case("lets ordinary pushes and other branches through", function()
  test.eq(call("git push origin main").blocked, false)
  test.eq(call("git push --force origin feature/x").blocked, false)
  test.eq(call("cargo test").blocked, false)
end)

test.case("ignores other tools", function()
  local r = test.emit("tool_call", { id = "c2", name = "read", args = { path = "main.rs" } })
  test.eq(r.blocked, false)
end)
