-- todo: a task list kept in the session (branch-aware: entries on the
-- current branch only), with a tool for the model, a /todo command for the
-- user, the status line and a widget.

local function load(psi)
  local items = {}
  for _, e in ipairs(psi.entries("todo")) do
    if e.op == "add" then
      items[#items + 1] = { text = e.text, done = false }
    elseif e.op == "done" and items[e.index] then
      items[e.index].done = true
    end
  end
  return items
end

local function show(psi, items)
  local open, lines = 0, {}
  for i, it in ipairs(items) do
    if not it.done then open = open + 1 end
    local mark = it.done and psi.style.dim("[x]") or "[ ]"
    lines[#lines + 1] = string.format("%s %d. %s", mark, i, it.text)
  end
  psi.ui.status("todo", open > 0 and (open .. " open") or nil)
  psi.ui.widget("todo", #lines > 0 and lines or nil)
  return lines
end

return {
  name = "todo",
  version = "0.1.0",
  description = "A task list in the session",
  setup = function(psi)
    psi.register_tool{
      name = "todo",
      description = "Keep a task list for this session. action is add (with text), done (with index) or list.",
      params = {
        action = { type = "string", description = "add, done or list" },
        text = { type = "string", description = "The task, for add" },
        index = { type = "integer", description = "The task number, for done" },
      },
      required = { "action" },
      execute = function(args)
        local items = load(psi)
        if args.action == "add" then
          if not args.text or args.text == "" then return { text = "add needs text", is_error = true } end
          psi.append_entry("todo", { op = "add", text = args.text })
          items[#items + 1] = { text = args.text, done = false }
        elseif args.action == "done" then
          if not items[args.index or 0] then
            return { text = "no task " .. tostring(args.index) .. "; there are " .. #items, is_error = true }
          end
          psi.append_entry("todo", { op = "done", index = args.index })
          items[args.index].done = true
        elseif args.action ~= "list" then
          return { text = "action is add, done or list, not " .. tostring(args.action), is_error = true }
        end
        local lines = show(psi, items)
        return #lines > 0 and table.concat(lines, "\n") or "(no tasks)"
      end,
    }
    psi.register_command("todo", {
      description = "Show the task list, or add a task: /todo <text>",
      handler = function(args, ctx)
        if args ~= "" then psi.append_entry("todo", { op = "add", text = args }) end
        local lines = show(psi, load(psi))
        ctx.ui.notify(#lines > 0 and table.concat(lines, "\n") or "no tasks")
      end,
    })
    psi.register_command("todo-next", {
      description = "Ask the agent to work on the first open task",
      handler = function()
        for i, it in ipairs(load(psi)) do
          if not it.done then
            psi.send_user_message("Work on task " .. i .. ": " .. it.text .. ". Mark it done with the todo tool when it is.")
            return
          end
        end
        psi.ui.notify("no open tasks")
      end,
    })
    psi.register_command("todo-share", {
      description = "Show the task list to the model with the next request",
      handler = function()
        local lines = show(psi, load(psi))
        psi.send_message("todo", "The user's task list:\n" .. (#lines > 0 and table.concat(lines, "\n") or "(empty)"))
      end,
    })
    -- Keep the status line current when the session changes.
    psi.on("session_start", function() show(psi, load(psi)) end)
  end,
}
