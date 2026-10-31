return {
  name = "throws",
  version = "0.1.0",
  description = "Setup throws",
  setup = function(psi)
    psi.on("input", function() end) print("before")
    error("boom")
  end,
}
