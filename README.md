# FlyBase AI Tools

Tools for using FlyBase data from AI assistants.

- `mcp/`: the hosted FlyBase MCP server (Rust). Endpoint: `https://mcp.flybase.org/mcp`.
- `plugins/flybase/`: a Claude Code plugin that registers the MCP server. Skills will live here later.

## Connect a client

- Claude Code: `claude mcp add --transport http flybase https://mcp.flybase.org/mcp`
- Codex: `codex mcp add flybase --url https://mcp.flybase.org/mcp`
- Claude.ai and Claude Desktop: Settings, Connectors, add a custom connector with the URL above.
- ChatGPT: Settings, Connectors, add an MCP server with the URL above.

## Develop

See `docs/deployment.md` for running the server and `mcp/README.md` for the crate.
