-- git-guard: blocks force pushes to main and master (a tool_call gate).
--
-- Psi already asks before any `git push`; this shows a gate that refuses
-- outright, with a reason the model reads.

local PROTECTED = { main = true, master = true }

local function force_push_target(command)
  if not command:find("git") or not command:find("push") then return nil end
  local force = command:find("%-%-force") or command:find("%s%-f%f[%s]") or command:find("%s%+%w")
  if not force then return nil end
  for branch in pairs(PROTECTED) do
    if command:find("%f[%w]" .. branch .. "%f[%W]") then return branch end
  end
  return nil
end

return {
  name = "git-guard",
  version = "0.1.0",
  description = "Blocks force pushes to main and master",
  setup = function(psi)
    psi.on("tool_call", function(ev, ctx)
      if ev.name ~= "bash" then return nil end
      local branch = force_push_target(ev.args.command or "")
      if branch then
        psi.log("blocked a force push to " .. branch)
        return { block = true, reason = "force-pushing " .. branch .. " is not allowed here; push a branch and open a pull request" }
      end
      return nil
    end)
  end,
}
