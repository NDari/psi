-- Held-out tests for trim_output (the model never sees them).

local function lines(n, first)
  local t = {}
  for i = first or 1, (first or 1) + n - 1 do t[#t + 1] = "line " .. i end
  return t
end

local function text(n) return table.concat(lines(n), "\n") end

-- One trailing newline is a fair reading of "lines"; it is ignored.
local function result(name, body, is_error)
  local p = test.emit("tool_result", {
    id = "1", name = name, args = {}, text = body, is_error = is_error or false,
  }).payload
  if type(p.text) == "string" then p.text = p.text:gsub("\n$", "") end
  return p
end

local function trimmed(n)
  local t = lines(10)
  t[#t + 1] = string.format("[... %d lines trimmed ...]", n - 20)
  for _, l in ipairs(lines(10, n - 9)) do t[#t + 1] = l end
  return table.concat(t, "\n")
end

test.case("41 lines are trimmed by default", function()
  test.eq(result("bash", text(41)).text, trimmed(41))
end)

test.case("40 lines are left alone", function()
  test.eq(result("bash", text(40)).text, text(40))
end)

test.case("a long result of another tool is left alone", function()
  test.eq(result("read", text(200)).text, text(200))
end)

test.case("an error result is trimmed and stays an error", function()
  local p = result("bash", text(75), true)
  test.eq(p.text, trimmed(75))
  test.eq(p.is_error, true, "is_error")
end)

test.case("a trimmed ordinary result is not an error", function()
  test.eq(result("bash", text(60)).is_error, false, "is_error")
end)

test.case("the max_lines flag raises the limit", function()
  test.set_flag("max_lines", "100")
  test.eq(result("bash", text(100)).text, text(100))
  test.eq(result("bash", text(101)).text, trimmed(101))
end)

test.case("the max_lines flag lowers the limit", function()
  test.set_flag("max_lines", "25")
  test.eq(result("bash", text(26)).text, trimmed(26))
end)
