# FlyBase AI Tools

Tools for using FlyBase data from AI assistants.

- `mcp/`: the hosted FlyBase MCP server (Rust). Endpoint: `https://mcp.flybase.org/mcp`.
- `plugins/flybase/`: the FlyBase plugin for Claude Code, Codex, and Google Antigravity.
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

In a new session, ask “What is the FlyBase ID for wingless?” You can also run `claude mcp list`
to check the connection. The plugin installs at user scope.

**Enable automatic plugin updates.** In Claude Code, open `/plugin`, select **Marketplaces**, choose
**flybase**, and turn on auto-update. Third-party marketplaces start with this setting off.
Run `/reload-plugins` when Claude Code reports an update, or start a new session. For a manual
refresh, run these commands in your shell. [Claude Code plugin updates](https://code.claude.com/docs/en/discover-plugins#keep-plugins-updated)

```bash
claude plugin marketplace update flybase
claude plugin update flybase@flybase
```

The plugin registers a hosted endpoint. FlyBase data releases and new server tools become available
when your client reconnects. Plugin updates deliver changes to the client package.

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

Codex can install the same plugin from FlyBase’s marketplace. Its CLI recognizes the Claude-compatible
catalog in this repository. [Codex marketplace guide](https://developers.openai.com/plugins/build/plugins)

```bash
codex plugin marketplace add https://github.com/FlyBase/flybase-ai-tools.git
codex plugin add flybase@flybase
codex mcp list
```

To update, run `codex plugin marketplace upgrade flybase`, then `codex plugin add flybase@flybase`
and start a new session. For a direct MCP connection instead, use the command below.

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

### Google Antigravity

Install the plugin directly from its directory in the public FlyBase repository:

```bash
agy plugin install https://github.com/FlyBase/flybase-ai-tools/tree/master/plugins/flybase
agy plugin list
```

Repeat the install command to replace the installed copy with the current version.
For a direct connection instead, run `agy mcp add --type http flybase https://mcp.flybase.org/mcp`.
[Google Antigravity plugin guide](https://antigravity.google/docs/ide/plugins/)

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
- **Batch downloads.** Submit up to 50,000 FlyBase IDs as one ordered list. For lists over 500 IDs,
  retrieve the JSON or TSV in pages of up to 1,000 rows. Repeat the call with the same IDs and fields,
  setting `offset` to the returned `next_offset` until it is null. See [the batch example](docs/examples.md#reports-batch-downloads-and-sequences).
- **Fair use.** The server is shared and rate limited per client. Requests over the limit get HTTP 429;
  wait and retry. One assistant session stays well within it. Please be polite with scripts: pause
  between batches, use `batch_download` and `validate_ids` for bulk work, and cache what you fetch.
- **Only `/mcp` is public.** `/health` and `/metrics` are intentionally not exposed by the public endpoint.

## BLAST

The MCP's BLAST tools are clients of a private job API. `blast-jobs/` implements it. The service is a
small Rust process that runs NCBI BLAST+ 2.17.0 against a directory of BLAST databases described by a
catalogue file, in its own container with the databases mounted read-only. Nothing in it is specific to
FlyBase, so anyone with BLAST databases and a catalogue can run it. Configuration, the image and the run
shape are in [docs/blast-jobs.md](docs/blast-jobs.md).

## Develop

The MCP integration suite starts a local HTTP server and exercises the tools against a PostgreSQL fixture
built from public Chado records. For example, it checks that the *white* gene’s minus-strand coding
sequence translates to its reported protein and that a 50,000-ID download preserves row order.
[Build the fixture and run the suite](fixture/README.md).

See the [deployment guide](docs/deployment.md) for running the server and the [crate README](mcp/README.md) for development.

Private-side handoff: [docs/handoff-private-side.md](docs/handoff-private-side.md) lists what a host must provide
(database, runtime role, release runbook step, proxy and CDN requirements, and the BLAST job API contract).

## Licence

The code in this repository is under the MIT License; see [LICENSE](LICENSE). The bundled release 5 to 6
coordinate map, `mcp/data/coordinates/dmel_r5_to_r6.tsv`, is derived from FlyBase's published map and
carries its BSD-3-Clause notice in
[mcp/data/coordinates/LICENSE.bulkfile-scripts](mcp/data/coordinates/LICENSE.bulkfile-scripts).
