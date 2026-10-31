return {
  name = "runaway-setup",
  version = "1",
  description = "A loop around pcall in setup",
  setup = function()
    while true do pcall(function() while true do end end) end
  end,
}
