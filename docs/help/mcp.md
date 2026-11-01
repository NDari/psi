MCP servers: tools from Model Context Protocol servers, offered to the model like psi's own.

Add servers to ~/.psi/mcp.json or, in a trusted project, .psi/mcp.json (a project entry
replaces a user one of the same name). The format is the mcpServers object Claude Code,
Cursor and pi use, so their entries can be pasted as they are:

  {
    "mcpServers": {
      "fs": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
      "docs": { "url": "https://example.com/mcp", "headers": { "Authorization": "Bearer ${DOCS_TOKEN}" } }
    }
  }

command, args, env and cwd start a local (stdio) server; url and headers reach a remote
one over streamable HTTP (not SSE, and no OAuth sign-in yet: put a token in a header).
${VAR} and ${VAR:-default} take values from the environment. Psi's own fields:
- "tools": ["a", "b"] offers only these; "disabledTools": [...] leaves these out.
- "approval": "auto" (default: a tool the server marks read-only runs, any other asks),
  "ask" or "never"; or per tool: { "*": "ask", "search": "never" }.
- "timeout": seconds per call (default 60). "enabled": false keeps an entry unused.

Tools appear as mcp__<server>__<tool>. Every tool's definition goes out with every
request, so keep the list short: psi warns when MCP tools take more than a tenth of the
context. Servers connect when psi starts; the first prompt waits up to 10 seconds for
them, and one that connects later joins at the next prompt. Plan mode allows only
tools marked read-only; the scout sub-agent gets no MCP tools, worker gets them all.

psi mcp list connects to every server and shows its tools and their size in tokens,
or why it failed; /mcp shows the same in a session, with how many calls failed and the
server's last stderr lines once one has (a server can stay connected while its calls fail).
