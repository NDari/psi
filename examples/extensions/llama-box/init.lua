-- llama-box: a second llama-server as a provider (psi.register_provider).
-- `psi --model box/qwen3.6-35b` talks to it; set BOX_KEY if it wants a key.

return {
  name = "llama-box",
  version = "0.1.0",
  description = "A llama-server on another machine",
  setup = function(psi)
    psi.register_provider("box", {
      base_url = "http://127.0.0.1:9932/v1",
      api_key = "$BOX_KEY",
      models = {
        { id = "qwen3.6-35b", context_window = 65536, max_output = 8192, enable_thinking = true },
      },
    })
  end,
}
