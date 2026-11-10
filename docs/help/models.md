Which model psi talks to, and how to add one.

psi talks to an OpenAI-compatible server, first of all llama-server. Without a
~/.psi/models.jsonc it uses llama-server on http://127.0.0.1:9931/v1. A profile:

{ "models": [{ "id": "qwen27b", "base_url": "http://127.0.0.1:9931/v1",
  "context_window": "auto", "max_output": 8192,
  "reasoning": { "enable_thinking": false },
  "sampling": { "temperature": 0.7, "top_p": 0.8, "top_k": 20, "min_p": 0.0,
                "presence_penalty": 1.5, "repeat_penalty": 1.0 } }] }

The first profile is the default; --model <id> picks another. The context window comes
from the server's /props when "auto". The server's chat template must support tool
calls (llama-server --jinja).

sampling holds the model card's values, by the server's own field names, and goes out
with every request, so a model's sampling lives with the model and not in the command
that starts the server. Leave it out and the server's settings apply.

Trying a new model: add a profile, start the server, then
psi eval run --tasks trial --model <id> (evals/README.md, "Trying a new model").
