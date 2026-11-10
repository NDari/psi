Which model psi talks to, and how to add one.

psi talks to an OpenAI-compatible server, first of all llama-server. Without a
~/.psi/models.jsonc it uses llama-server on http://127.0.0.1:9931/v1. A profile:

{ "models": [{ "id": "qwen", "base_url": "http://127.0.0.1:9931/v1",
  "context_window": "auto", "max_output": 8192,
  "reasoning": { "enable_thinking": false } }] }

The first profile is the default; --model <id> picks another. The context window comes
from the server's /props when "auto". The server's chat template must support tool
calls (llama-server --jinja).
