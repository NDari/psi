local LOG = table.concat({
  "@a1b2c3d 2026-11-01T10:00:00+00:00 Fix   the  parser (#12)",
  "",
  "src/parse.rs",
  "README.md",
  "@d4e5f6a 2026-10-30T09:00:00+00:00 Add docs",
  "",
  "docs/guide.md",
  "",
}, "\n")

test.case("the tool lists commits with the files they touched", function()
  test.ui.answer("exec", { code = 0, stdout = LOG })
  local r = test.tool("recent_changes", { days = 3 })
  test.eq(r.is_error, false)
  test.contains(r.text, "a1b2c3d 11-01 Fix the parser (#12) [rs in src, md in .] refs #12")
  test.contains(r.text, "d4e5f6a 10-30 Add docs [md in docs]")
  local call = test.requests("exec")[1]
  test.eq(call.argv[1], "git")
  test.eq(call.argv[2], "log")
end)

test.case("under keeps the commits touching that directory", function()
  test.ui.answer("exec", { code = 0, stdout = LOG })
  local r = test.tool("recent_changes", { under = "docs" })
  test.contains(r.text, "d4e5f6a")
  test.not_contains(r.text, "a1b2c3d")
end)

test.case("a failing git is a failed tool call", function()
  test.ui.answer("exec", { code = 128, stderr = "fatal: not a git repository" })
  local r = test.tool("recent_changes", {})
  test.truthy(r.is_error)
  test.contains(r.text, "git log failed: fatal: not a git repository")
end)

test.case("the report is JSON inside the project", function()
  test.ui.answer("exec", { code = 0, stdout = LOG })
  test.command("/changes-save", "out/changes.json")
  test.eq(test.requests("notify")[1].message, "saved 2 commits to out/changes.json")
  local saved = psi.json.decode(psi.fs.read(psi.path.join(test.tmp, "out/changes.json")))
  test.eq(saved.commits[1].files, { "src/parse.rs", "README.md" })
  test.eq(saved.commits[2].subject, "Add docs")
end)

test.case("a path outside the project is refused", function()
  test.command("/changes-save", "../elsewhere.json")
  test.eq(test.requests("notify")[1].message, "keep the report inside the project")
  test.eq(#test.requests("exec"), 0)
end)
