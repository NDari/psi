-- Held-out tests for lockfile_guard (the model never sees them).

local function call(name, args)
  return test.emit("tool_call", { id = "1", name = name, args = args })
end

local function blocked_with(r, path)
  test.truthy(r.blocked, "blocked")
  test.eq(r.payload.reason, "lockfiles are generated, not edited: " .. path, "reason")
end

test.case("write Cargo.lock is blocked", function()
  blocked_with(call("write", { path = "Cargo.lock", text = "x" }), "Cargo.lock")
end)

test.case("edit of a nested package-lock.json is blocked", function()
  local p = "web/app/package-lock.json"
  blocked_with(call("edit", { path = p, old_text = "a", new_text = "b" }), p)
end)

test.case("write go.sum by absolute path is blocked", function()
  local p = "/work/project/go.sum"
  blocked_with(call("write", { path = p, text = "" }), p)
end)

test.case("read of a lockfile goes through", function()
  test.eq(call("read", { path = "Cargo.lock" }).blocked, false)
end)

test.case("bash on a lockfile goes through", function()
  test.eq(call("bash", { command = "rm Cargo.lock" }).blocked, false)
end)

test.case("names that only look like lockfiles go through", function()
  for _, p in ipairs({ "notgo.sum", "go.sum.txt", "my.lock.bak", "lock", "src/lockfile.rs", "package-lock.json.orig" }) do
    test.eq(call("write", { path = p, text = "" }).blocked, false, p)
  end
end)

test.case("a directory named .lock does not block its files", function()
  test.eq(call("write", { path = "x.lock/readme.md", text = "" }).blocked, false)
end)
