# FlyBase AI Tools

Tools for using FlyBase data from AI assistants.

- `mcp/`: the hosted FlyBase MCP server (Rust). Endpoint: `https://mcp.flybase.org/mcp`.
- `plugins/flybase/`: a Claude Code plugin that registers the MCP server. Skills will live here later.
- `blast-jobs/`: the BLAST job service behind the MCP's BLAST tools (Rust, NCBI BLAST+ 2.17.0).

The server's tools and their arguments are listed in [docs/tools.md](docs/tools.md).

## Install

The server is public at `https://mcp.flybase.org/mcp`. It speaks the Model Context Protocol over Streamable
HTTP and needs no account, key or login. Pick your client below.

### Claude Code, through the plugin marketplace

Inside a Claude Code session:

```text
/plugin marketplace add FlyBase/flybase-ai-tools
/plugin install flybase@flybase
```

The same two steps work from a shell:

```bash
claude plugin marketplace add FlyBase/flybase-ai-tools
claude plugin install flybase@flybase
```

The plugin installs at user scope and registers the server. `claude mcp list` then shows
`plugin:flybase:flybase: https://mcp.flybase.org/mcp (HTTP) - ✔ Connected`. Plugin tools are named
`mcp__plugin_flybase_flybase__<tool>`. Skills will ship in the same plugin later.

### Claude Code, without the plugin

```bash
claude mcp add --transport http flybase https://mcp.flybase.org/mcp
```

That adds the server for the current project on your machine only. Add `--scope user` for every project.
To share it with everyone who clones a project, use `--scope project`, or commit this `.mcp.json` at the
project root:

```json
{
  "mcpServers": {
    "flybase": {
      "type": "http",
      "url": "https://mcp.flybase.org/mcp"
    }
  }
}
```

Claude Code asks each user to approve a project's `.mcp.json` servers the first time it starts there.
For a one-off non-interactive run, save the same JSON as `flybase.json` and pass it in. Put the prompt
before `--allowedTools`, which takes a list and would otherwise swallow it:

```bash
claude -p "What is the FlyBase ID for wingless?" --mcp-config flybase.json --allowedTools "mcp__flybase__*"
```

### Codex

```bash
codex mcp add flybase --url https://mcp.flybase.org/mcp
```

Or add this to `~/.codex/config.toml`:

```toml
[mcp_servers.flybase]
url = "https://mcp.flybase.org/mcp"
```

`codex exec` with an approval policy of `never` refuses MCP tool calls that need approval. The tools only
read FlyBase data (`blast_submit` starts a search job and nothing else), so for scripted runs you can
approve them up front. Add this line under the existing `[mcp_servers.flybase]` table, below its `url`
line (do not repeat the table header):

```toml
default_tools_approval_mode = "approve"
```

### Claude.ai and Claude Desktop

Add a custom connector: open Settings, then Connectors, choose "Add custom connector", paste
`https://mcp.flybase.org/mcp`, and leave the OAuth fields empty. Free plans allow one custom connector. On
Team and Enterprise plans an owner adds the connector for the organization first, then each member connects. Enable it per chat from the "+" menu under Connectors. See
Anthropic's [custom connector guide](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp).

### ChatGPT

Open ChatGPT's Plugins page (chatgpt.com/plugins), choose the plus button, then "Add custom MCP server". Give
it a name such as FlyBase, choose "Public endpoint" and enter `https://mcp.flybase.org/mcp`, and set no
authentication. Confirm the warning and choose "Create as a plugin". In a new chat, type `@` and pick it.
Workspace policies decide whether custom servers are allowed, and older accounts reach the same form through
developer mode under Settings, Apps. OpenAI's
[connect from ChatGPT guide](https://developers.openai.com/apps-sdk/deploy/connect-chatgpt) has the current
steps.

### Any other MCP client

| Setting | Value |
|---|---|
| URL | `https://mcp.flybase.org/mcp` |
| Transport | Streamable HTTP (JSON-RPC over `POST`) |
| Authentication | none |
| Tools | 25; they read FlyBase data and never change it |

`initialize` is optional, so a bare `tools/call` works:

```bash
curl -s https://mcp.flybase.org/mcp \
  -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"resolve","arguments":{"query":"wingless"}}}'
```

## What you can ask

Ask in plain language. For example:

- "What is the best human ortholog of wingless?"
- "Which genes are highly expressed in adult testis but off in the ovary?"
- "What genes and aberrations are in cytological band 89D?"
- "I want a GAL4 driver for wingless. What can I order?"
- "BLAST this protein fragment against D. melanogaster polypeptides and tell me what it is."

[docs/examples.md](docs/examples.md) has tested questions for every tool, with the call the assistant makes
and what comes back. The server's 25 tools cover identifying symbols and IDs, reports, batch downloads, the
genome (sequence, features in a range, cytology, release 5 to 6 coordinates), controlled vocabularies,
homepage data (orthologs, gene groups, disease models, stocks, interactions), RNA-seq expression, BLAST, and
the FlyBase wiki's help pages (what report fields mean, how tools and downloads work).
Each is described in [docs/tools.md](docs/tools.md).

## Using the public server

- **Data release.** Every result names the FlyBase release it came from, currently FB2026_03.
- **BLAST databases.** BLAST searches the archived FB2023_04 database set, so BLAST results can lag the rest of
  the server. Each `blast_submit` response says so.
- **Rate limit.** Each client IP gets about 2 requests per second, with a short burst allowance. Requests over
  the limit get HTTP 429. One assistant session stays well under it. Please be polite with scripts: pause
  between calls and use `batch_download` for long ID lists.
- **Only `/mcp` is public.** `/health` and `/metrics` are intentionally not exposed and return 404.

## BLAST

The MCP's BLAST tools are clients of a private job API. `blast-jobs/` implements it. The service is a
small Rust process that runs NCBI BLAST+ 2.17.0 against a directory of BLAST databases described by a
catalogue file, in its own container with the databases mounted read-only. Nothing in it is specific to
FlyBase, so anyone with BLAST databases and a catalogue can run it. Configuration, the image and the run
shape are in [docs/blast-jobs.md](docs/blast-jobs.md).

## Develop

See `docs/deployment.md` for running the server and `mcp/README.md` for the crate.

Private-side handoff: [docs/handoff-private-side.md](docs/handoff-private-side.md) lists what a host must provide
(database, runtime role, release runbook step, proxy and CDN requirements, and the BLAST job API contract).

## Licence

The code in this repository is under the MIT License; see [LICENSE](LICENSE). The bundled release 5 to 6
coordinate map, `mcp/data/coordinates/dmel_r5_to_r6.tsv`, is derived from FlyBase's published map and
carries its BSD-3-Clause notice in
[mcp/data/coordinates/LICENSE.bulkfile-scripts](mcp/data/coordinates/LICENSE.bulkfile-scripts).
