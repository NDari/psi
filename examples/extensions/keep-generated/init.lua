-- keep-generated: generated files are rebuilt by a command, not edited by
-- hand (a tool_call gate).
--
-- Writes and edits to them are refused with the command to run instead.
-- Bash commands that name one are refused only when they may write, so the
-- model can still read the file with cat, grep or git diff: psi.bash.writes
-- answers nil for a command that only reads.

local GENERATED = {
  ["docs/api.md"] = "just api-docs",
}

local function refuse(path, command)
  return {
    block = true,
    reason = path .. " is generated; don't edit it. Run `" .. command .. "` to rebuild it.",
  }
end

return {
  name = "keep-generated",
  version = "0.1.0",
  description = "Refuses edits to generated files; reads stay allowed",
  setup = function(psi)
    psi.on("tool_call", function(ev, ctx)
      for path, command in pairs(GENERATED) do
        if ev.name == "write" or ev.name == "edit" then
          local p = ev.args.path or ""
          if p:sub(1, 1) ~= "/" then p = psi.path.join(ctx.cwd, p) end
          if psi.path.relative(p, ctx.cwd) == path then return refuse(path, command) end
        elseif ev.name == "bash" then
          local cmd = ev.args.command or ""
          local names_it = cmd:find(path, 1, true)
          if names_it and cmd ~= command and psi.bash.writes(cmd) then
            return refuse(path, command)
          end
        end
      end
      return nil
    end)
  end,
}
