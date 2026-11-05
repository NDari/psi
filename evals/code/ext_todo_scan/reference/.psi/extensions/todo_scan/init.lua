local function split(s, sep)
  local parts = {}
  for p in (s .. sep):gmatch("(.-)" .. sep) do parts[#parts + 1] = p end
  return parts
end

-- One path segment against one glob segment: * and ? stay inside it.
local function segment_pattern(g)
  local p = g:gsub("[%^%$%(%)%%%.%[%]%+%-]", "%%%0"):gsub("%*", "[^/]*"):gsub("%?", "[^/]")
  return "^" .. p .. "$"
end

local function match(globs, gi, parts, pi)
  if gi > #globs then return pi > #parts end
  if globs[gi] == "**" then
    for k = pi, #parts do
      if match(globs, gi + 1, parts, k) then return true end
    end
    return false
  end
  if pi > #parts then return false end
  return parts[pi]:match(segment_pattern(globs[gi])) ~= nil and match(globs, gi + 1, parts, pi + 1)
end

local fs -- psi.fs, set in setup

-- Every file under dir, relative to it, sorted (io.popen is gone since 3db400e).
local function files_under(dir)
  return fs.glob("**", { cwd = dir, gitignore = false })
end

local function scan(args, ctx)
  local limit = args.limit or 50
  local globs = split(args.glob, "/")
  local found = {}
  for _, rel in ipairs(files_under(ctx.cwd)) do
    if match(globs, 1, split(rel, "/"), 1) then
      local f = io.open(ctx.cwd .. "/" .. rel, "rb")
      local body = f and f:read("a")
      if f then f:close() end
      if body and not body:find("\0", 1, true) then
        local n = 0
        for line in (body .. "\n"):gmatch("(.-)\n") do
          n = n + 1
          if line:find("TODO", 1, true) then
            found[#found + 1] = string.format("%s:%d: %s", rel, n, line:match("^%s*(.-)%s*$"))
          end
        end
      end
    end
  end
  if #found == 0 then return "no TODOs" end
  local out = {}
  for i = 1, math.min(limit, #found) do out[#out + 1] = found[i] end
  if #found > limit then out[#out + 1] = string.format("(%d more)", #found - limit) end
  return table.concat(out, "\n")
end

return {
  name = "todo_scan",
  version = "0.1.0",
  description = "A tool that lists TODO comments",
  setup = function(psi)
    fs = psi.fs
    psi.register_tool({
      name = "todo_scan",
      description = "List lines containing TODO in the files matching a glob",
      params = {
        glob = { type = "string", description = "Files to scan, relative to the working directory, like **/*.py" },
        limit = { type = "integer", description = "The most matches to list (default 50)" },
      },
      required = { "glob" },
      annotations = { read_only = true },
      execute = scan,
    })
  end,
}
