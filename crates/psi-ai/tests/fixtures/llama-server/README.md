# Recorded llama-server responses

Raw HTTP responses from llama-server b11429 (2026-10-31), one directory per
reference model, recorded with an 8192-token context so the overflow error
is cheap to trigger. `server-cmd.txt` is the exact server command.

Each `<name>.request.json` was sent to `/v1/chat/completions` and
`<name>.response.http` is the response byte for byte (status line, headers,
chunked body). The replay server writes them back verbatim, so the client is
tested against real bytes.

| Name | What it shows |
|---|---|
| `plain` | A streamed text answer |
| `plain_nostream` | The same, without streaming |
| `tool_calls` | Tool calls streamed in pieces (Qwen makes two in parallel, Gemma one) |
| `after_tools` | An answer after a tool result |
| `length_text` | `finish_reason: length` in text |
| `length_tool_call` | `finish_reason: length` in the middle of a tool call's arguments |
| `overflow` | HTTP 400 `exceed_context_size_error` |
| `system_not_first` | Qwen: HTTP 500 from the chat template; Gemma accepts it |
| `props`, `health`, `models`, `tokenize` | The other endpoints |
