return {
  name = "bad-tool",
  version = "0.1.0",
  description = "A tool spec without execute",
  setup = function(psi)
    psi.register_tool({ name = "x", description = "y", params = {} })
  end,
}
