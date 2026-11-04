-- changes: what changed lately, from git. A read-only tool for the model
-- and a command that saves a report. Shows psi.exec (it runs inside the
-- sandbox, and asks for commands on the always-ask list), psi.re,
-- psi.time, psi.path, psi.fs.write and psi.json.

local DAY = 86400

-- The commits of the last `days` days, newest first.
local function commits(psi, cwd, days)
  local since = psi.time.format(psi.time.now() - days * DAY, "%Y-%m-%d")
  local r = psi.exec(
    { "git", "log", "--since=" .. since, "--name-only", "--format=@%h %cI %s" },
    { cwd = cwd, timeout_ms = 10000 }
  )
  if not r.ok then error("git log failed: " .. r.stderr) end
  local list = {}
  for _, line in ipairs(psi.re.split("\n", r.stdout)) do
    local m = psi.re.find("^@(\\w+) (\\S+) (.*)$", line)
    if m then
      list[#list + 1] = {
        hash = m.groups[1],
        when = psi.time.parse(m.groups[2]),
        subject = psi.re.replace("\\s+", m.groups[3], " "),
        -- An array even when empty, for psi.json.encode.
        files = psi.json.array({}),
      }
    elseif line ~= "" and #list > 0 then
      table.insert(list[#list].files, line)
    end
  end
  return list
end

-- "src/parse.rs" reads "rs in src".
local function kind(psi, file)
  local ext = psi.path.ext(file)
  local name = ext ~= "" and ext or psi.path.basename(file)
  return name .. " in " .. psi.path.dirname(psi.path.normalize(file))
end

local function summary(psi, list)
  local lines = {}
  for _, c in ipairs(list) do
    local kinds, refs = {}, {}
    for _, f in ipairs(c.files) do kinds[#kinds + 1] = kind(psi, f) end
    for _, m in ipairs(psi.re.find_all("#\\d+", c.subject)) do refs[#refs + 1] = m.text end
    lines[#lines + 1] = string.format("%s %s %s [%s]%s", c.hash, psi.time.format(c.when, "%m-%d"),
      c.subject, table.concat(kinds, ", "), #refs > 0 and (" refs " .. table.concat(refs, " ")) or "")
  end
  return table.concat(lines, "\n")
end

return {
  name = "changes",
  version = "0.1.0",
  description = "Recent commits for the model, and a saved report",
  setup = function(psi)
    psi.register_tool{
      name = "recent_changes",
      description = "List the commits of the last few days with the files each touched",
      params = {
        days = { type = "integer", description = "How many days back (default 7)" },
        under = { type = "string", description = "Only commits touching files under this directory" },
      },
      required = {},
      annotations = { read_only = true },
      execute = function(args, ctx)
        local list = commits(psi, ctx.cwd, args.days or 7)
        if args.under then
          -- escape: the directory is matched as text, so "a.b" doesn't match "aXb".
          local prefix = "^" .. psi.re.escape(psi.path.normalize(args.under) .. "/")
          local kept = {}
          for _, c in ipairs(list) do
            for _, f in ipairs(c.files) do
              if psi.re.is_match(prefix, f) then kept[#kept + 1] = c; break end
            end
          end
          list = kept
        end
        if #list == 0 then return "no commits in that time" end
        return summary(psi, list)
      end,
    }
    psi.register_command("changes-save", {
      description = "Save the last week's commits as JSON (default changes.json)",
      handler = function(args, ctx)
        local name = args ~= "" and args or "changes.json"
        local target = psi.path.absolute(psi.path.join(ctx.cwd, name))
        local shown = psi.path.relative(target, ctx.cwd)
        if psi.re.is_match("^\\.\\.", shown) then
          return ctx.ui.notify("keep the report inside the project", "error")
        end
        local list = commits(psi, ctx.cwd, 7)
        psi.fs.write(target, psi.json.encode({ made = psi.time.format(), commits = list }, { pretty = true }))
        ctx.ui.notify("saved " .. #list .. " commits to " .. shown)
      end,
    })
  end,
}
