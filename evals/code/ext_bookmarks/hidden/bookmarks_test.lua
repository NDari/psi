-- Held-out tests for bookmarks (the model never sees them). Each case
-- starts with no session entries.

local function bm(args)
  return test.tool("bookmark", args)
end

-- One trailing newline is a fair reading of "one line per bookmark".
local function trim(text) return (text:gsub("\n$", "")) end

local function ok(args, want)
  local r = bm(args)
  test.eq(trim(r.text), want)
  test.eq(r.is_error, false, "is_error")
end

local function last_marks()
  local reqs = test.requests("append_entry")
  local last = reqs[#reqs]
  test.truthy(last, "an appended entry")
  test.eq(last.custom_type, "bookmarks", "entry type")
  return last.data.marks
end

test.case("add, then list sorted by name", function()
  ok({ action = "add", name = "parser", note = "src/parse.py:40" }, "added parser")
  ok({ action = "add", name = "api", note = "the REST layer" }, "added api")
  ok({ action = "list" }, "api: the REST layer\nparser: src/parse.py:40")
end)

test.case("add replaces, and each change appends the whole set", function()
  ok({ action = "add", name = "a", note = "one" }, "added a")
  ok({ action = "add", name = "b", note = "two" }, "added b")
  ok({ action = "add", name = "a", note = "three" }, "added a")
  test.eq(last_marks(), { a = "three", b = "two" })
  ok({ action = "list" }, "a: three\nb: two")
end)

test.case("add needs name and note", function()
  for _, args in ipairs({ { action = "add", name = "x" }, { action = "add", note = "y" } }) do
    local r = bm(args)
    test.eq(r.text, "add needs name and note")
    test.eq(r.is_error, true, "is_error")
  end
end)

test.case("remove", function()
  ok({ action = "add", name = "a", note = "one" }, "added a")
  ok({ action = "add", name = "b", note = "two" }, "added b")
  ok({ action = "remove", name = "a" }, "removed a")
  test.eq(last_marks(), { b = "two" })
  ok({ action = "list" }, "b: two")
  local r = bm({ action = "remove", name = "zzz" })
  test.eq(r.text, "no bookmark zzz")
  test.eq(r.is_error, true, "is_error")
end)

test.case("an empty list", function()
  ok({ action = "list" }, "no bookmarks")
end)

test.case("the branch's last entry is the state", function()
  test.set_entries({
    { "bookmarks", { marks = { old = "gone" } } },
    { "other", { marks = { nope = "x" } } },
    { "bookmarks", { marks = { kept = "here", also = "this" } } },
  })
  ok({ action = "list" }, "also: this\nkept: here")
  ok({ action = "add", name = "new", note = "n" }, "added new")
  test.eq(last_marks(), { kept = "here", also = "this", new = "n" })
end)

test.case("/bookmarks shows the list", function()
  test.set_entries({ { "bookmarks", { marks = { b = "2", a = "1" } } } })
  test.command("/bookmarks")
  local n = test.requests("notify")
  test.eq(#n, 1, "notifications")
  test.eq(trim(n[1].message), "a: 1\nb: 2")
end)
