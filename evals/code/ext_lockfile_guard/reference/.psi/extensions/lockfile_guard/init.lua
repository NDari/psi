local LOCKFILES = { ["go.sum"] = true, ["package-lock.json"] = true }

local function is_lockfile(path)
  local name = path:match("[^/]*$")
  return LOCKFILES[name] or name:sub(-5) == ".lock"
end

return {
  name = "lockfile_guard",
  version = "0.1.0",
  description = "Blocks writes and edits of lockfiles",
  setup = function(psi)
    psi.on("tool_call", function(ev)
      if ev.name ~= "write" and ev.name ~= "edit" then return end
      local path = ev.args.path
      if type(path) == "string" and is_lockfile(path) then
        return { block = true, reason = "lockfiles are generated, not edited: " .. path }
      end
    end)
  end,
}
