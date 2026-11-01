local function current(psi)
  local list = psi.entries("bookmarks")
  local last = list[#list]
  local marks = {}
  for k, v in pairs(last and last.marks or {}) do marks[k] = v end
  return marks
end

local function listing(marks)
  local names = {}
  for k in pairs(marks) do names[#names + 1] = k end
  if #names == 0 then return "no bookmarks" end
  table.sort(names)
  local lines = {}
  for _, n in ipairs(names) do lines[#lines + 1] = n .. ": " .. marks[n] end
  return table.concat(lines, "\n")
end

return {
  name = "bookmarks",
  version = "0.1.0",
  description = "Named bookmarks kept in the session",
  setup = function(psi)
    psi.register_tool({
      name = "bookmark",
      description = "Keep named notes for this session. action is add (name, note), remove (name) or list.",
      params = {
        action = { type = "string", description = "add, remove or list" },
        name = { type = "string", description = "The bookmark's name" },
        note = { type = "string", description = "The note, for add" },
      },
      required = { "action" },
      execute = function(args)
        local marks = current(psi)
        if args.action == "add" then
          if not args.name or not args.note then
            return { text = "add needs name and note", is_error = true }
          end
          marks[args.name] = args.note
          psi.append_entry("bookmarks", { marks = marks })
          return "added " .. args.name
        elseif args.action == "remove" then
          if not args.name or marks[args.name] == nil then
            return { text = "no bookmark " .. tostring(args.name), is_error = true }
          end
          marks[args.name] = nil
          psi.append_entry("bookmarks", { marks = marks })
          return "removed " .. args.name
        elseif args.action == "list" then
          return listing(marks)
        end
        return { text = "action is add, remove or list", is_error = true }
      end,
    })
    psi.register_command("bookmarks", {
      description = "Show the bookmarks",
      handler = function(_, ctx)
        ctx.ui.notify(listing(current(psi)))
      end,
    })
  end,
}
