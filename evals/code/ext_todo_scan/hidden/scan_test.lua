-- Held-out tests for todo_scan (the model never sees them). The tool's
-- working directory is test.tmp.

-- psi.fs.write makes the directories (os.execute is gone since 3db400e).
local function put(rel, body)
  psi.fs.write(test.tmp .. "/" .. rel, body)
end

local function project()
  put("a.py", "import os\n  # TODO: handle errors  \nx = 1\n\nprint('TODO later')\n")
  put("src/b.py", "# TODO first\n")
  put("src/deep/c.py", "x = 2\ny = 3  # TODO: name it")
  put("src/deep/d.txt", "TODO not python\n")
  put("bin.py", "TODO\0binary\n")
  put("notes.md", "nothing to do\n")
end

local function scan(args)
  local r = test.tool("todo_scan", args)
  test.eq(r.is_error, false, "is_error (" .. tostring(r.text) .. ")")
  -- One trailing newline is a fair reading of "one line per match".
  return (r.text:gsub("\n$", ""))
end

local ALL = table.concat({
  "a.py:2: # TODO: handle errors",
  "a.py:5: print('TODO later')",
  "src/b.py:1: # TODO first",
  "src/deep/c.py:2: y = 3  # TODO: name it",
}, "\n")

test.case("** matches files at any depth, ordered by path then line", function()
  project()
  test.eq(scan({ glob = "**/*.py" }), ALL)
end)

test.case("* stays in one directory", function()
  project()
  test.eq(scan({ glob = "src/*.py" }), "src/b.py:1: # TODO first")
end)

test.case("** under a directory", function()
  project()
  test.eq(scan({ glob = "src/**/*.txt" }), "src/deep/d.txt:1: TODO not python")
end)

test.case("limit lists the first matches and counts the rest", function()
  project()
  test.eq(scan({ glob = "**/*.py", limit = 2 }),
    "a.py:2: # TODO: handle errors\na.py:5: print('TODO later')\n(2 more)")
end)

test.case("the default limit is 50", function()
  local lines = {}
  for i = 1, 53 do lines[i] = "TODO " .. i end
  put("many.txt", table.concat(lines, "\n"))
  local out = scan({ glob = "many.txt" })
  local n = select(2, out:gsub("\n", "\n")) + 1
  test.eq(n, 51, "lines")
  test.eq(out:match("[^\n]*$"), "(3 more)", "last line")
  test.eq(out:match("^[^\n]*"), "many.txt:1: TODO 1", "first line")
end)

test.case("no matches", function()
  project()
  test.eq(scan({ glob = "**/*.md" }), "no TODOs")
  test.eq(scan({ glob = "**/*.rs" }), "no TODOs")
end)

test.case("binary files are skipped", function()
  project()
  test.eq(scan({ glob = "bin.py" }), "no TODOs")
end)
