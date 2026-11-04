local function bash(command)
  return test.emit("tool_call", { id = "c1", name = "bash", args = { command = command } })
end

test.case("lets reads of a generated file through", function()
  test.falsy(bash("grep -n 'escape' docs/api.md").blocked)
  test.falsy(bash("cat docs/api.md | head -20").blocked)
  test.falsy(bash("git diff docs/api.md").blocked)
end)

test.case("refuses bash that may write it, naming the command to run", function()
  local r = bash("sed -i 's/a/b/' docs/api.md")
  test.truthy(r.blocked)
  test.contains(r.payload.reason, "Run `just api-docs`")
  test.truthy(bash("echo x > docs/api.md").blocked)
end)

test.case("lets the rebuild command run", function()
  test.falsy(bash("just api-docs").blocked)
end)

test.case("refuses write and edit calls on it", function()
  local r = test.emit("tool_call", { id = "c2", name = "edit", args = { path = "docs/api.md", old_text = "a", new_text = "b" } })
  test.truthy(r.blocked)
  local w = test.emit("tool_call", { id = "c3", name = "write", args = { path = "docs/other.md", text = "x" } })
  test.falsy(w.blocked)
end)
