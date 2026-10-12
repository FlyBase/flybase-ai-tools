# FlyBase AI Tools

Tools for using FlyBase data from AI assistants.

- `mcp/`: the hosted FlyBase MCP server (Rust). Endpoint: `https://mcp.flybase.org/mcp`.
- `plugins/flybase/`: a Claude Code plugin that registers the MCP server. Skills will live here later.

The server's tools and their arguments are listed in [docs/tools.md](docs/tools.md).

## Connect a client

- Claude Code: `claude mcp add --transport http flybase https://mcp.flybase.org/mcp`
- Codex: `codex mcp add flybase --url https://mcp.flybase.org/mcp`
- Claude.ai and Claude Desktop: Settings, Connectors, add a custom connector with the URL above.
- ChatGPT: Settings, Connectors, add an MCP server with the URL above.

The server's 23 tools cover identifying symbols and IDs, reports, batch downloads, the genome (sequence,
features in a range, cytology, release 5 to 6 coordinates), controlled vocabularies, homepage data (orthologs,
gene groups, disease models, stocks, interactions), RNA-seq expression, and BLAST. Each is described in
[docs/tools.md](docs/tools.md).

## Develop

See `docs/deployment.md` for running the server and `mcp/README.md` for the crate.

Private-side handoff: [docs/handoff-private-side.md](docs/handoff-private-side.md) lists what a host must provide
(database, runtime role, release runbook step, proxy and CDN requirements, and the BLAST job API contract).
