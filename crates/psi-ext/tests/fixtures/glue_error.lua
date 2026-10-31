return {
  name = "glue-error",
  version = "0.1.0",
  description = "A wrong argument the static pass can't see",
  setup = function(psi)
    local key = {} psi.ui.status(key, "x")
  end,
}
