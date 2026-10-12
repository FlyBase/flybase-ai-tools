# Deploying the FlyBase MCP server

The server is one Rust binary, `flybase-mcp`, packaged as a container. It reads a database on a dedicated
PostgreSQL 16 server (`chado-mcp`), restored from a FlyBase release dump, and serves the Model Context
Protocol over Streamable HTTP at `/mcp`. The public endpoint is `https://mcp.flybase.org/mcp`.

Private-side handoff: [handoff-private-side.md](handoff-private-side.md) is the checklist for what the hosting
side provides (database, runtime role, release runbook step, proxy and CDN requirements, BLAST job API).

## Environment

All variables are required unless marked optional.

| Variable | Example | Notes |
|---|---|---|
| `FLYBASE_MCP_BIND` | `127.0.0.1:7200` | Listen address. The image sets `0.0.0.0:7200`. |
| `FLYBASE_MCP_PUBLIC_URL` | `https://mcp.flybase.org` | Sets the allowed `Host` and `Origin` values for `/mcp`. |
| `FLYBASE_RELEASE` | `FB2026_03` | Reported in every result and in `/health`. Chado has no release marker, so set it per load. |
| `CHADO_URL` | `postgres://flybase_mcp:<secret>@chado-mcp:5432/flybase` | Runtime role for `serve`; the owner role for `migrate`. |
| `CHADO_POOL_SIZE` | `32` | Optional, default 32. Keep it below the role's connection limit. |
| `BLAST_BASE_URL` | `http://blast-web:8080` | Optional. Base URL of the private BLAST job API (see `docs/handoff-private-side.md` for the contract). Must be `http://`: no TLS is compiled in, and an `https://` value is rejected at startup (logged, BLAST tools then answer as unconfigured). Unset: BLAST tools answer `backend_unavailable` and `/health` reports `blast: unconfigured`. |
| `BLAST_SERVICE_TOKEN` | `<secret>` | Optional. Bearer token the BLAST API expects. A URL without a token is allowed and logged at warn. Never logged or returned. |
| `FLYBASE_MCP_RESPONSE_CACHE_MB` | `256` | Optional. Response cache bound in MiB; 0 disables it. |
| `RUST_LOG` | `info` | Log filter. The image sets `info`. At `debug`, rmcp logs every tool call's arguments, including BLAST job tokens. |

`migrate` loads the same configuration as `serve`, so the bind, public URL, and release variables must be
set for it too. Secrets come from the environment at deploy time and never belong in the repository.

## Build the image

    docker build -t flybase-mcp mcp

The build stage uses `rust:1.90-bookworm`; the runtime stage is `debian:bookworm-slim` running as an
unprivileged `flybase` user (uid 10001). The image exposes 7200 and its `HEALTHCHECK` polls `/health`.
`deploy/docker-compose.sample.yml` is a sample service; publish the port on loopback only.

## Prepare the database

1. Restore the release dump into an empty database on the `chado-mcp` server as its owner role.
2. Create the runtime role once per cluster (roles survive a database rebuild):

   ```sql
   create role flybase_mcp login password '<secret>' connection limit 40;
   ```

3. Apply the migrations as the owner role. They are idempotent and run in numeric order:

   ```bash
   docker run --rm \
     -e FLYBASE_MCP_PUBLIC_URL=https://mcp.flybase.org -e FLYBASE_RELEASE=FB2026_03 \
     -e CHADO_URL='postgres://<owner>:<secret>@chado-mcp:5432/flybase' \
     flybase-mcp migrate
   ```

   The migrations create schema `mcp` (name index, cvterm closure, band coordinates, and, in `005_rnaseq`,
   the RNA-seq expression, dataset, and matrix tables, which take about 25 to 30 s on a full restore), add
   `pg_trgm` and trigram indexes, and finish with `007_grants`, which gives `flybase_mcp` `SELECT` and `EXECUTE` on what
   the server reads. Each one is recorded in `mcp.migration_log` with the crate version and release.
   Rebuilding the derived tables takes a few minutes on a full load.

   Download `precomputed_files/orthologs/dmel_human_orthologs_disease_fb_<release>.tsv.gz` from the release
   FTP and run `flybase-mcp migrate --diopt <path>`; without it `get_orthologs` still works (scores from
   chado) but has no OMIM phenotypes.

4. Start `serve` with the runtime role. At startup the server checks that the role can read every relation
   its queries use and that every migration in the binary is logged. It refuses to start otherwise, so a
   load that skipped `migrate` fails at deploy rather than at query time.

Each pooled connection sets `default_transaction_read_only = on`, a 10 s `statement_timeout`, and a
`search_path` starting with `mcp`. The server has no write path.

For each release: restore the dump, run `migrate`, bump `FLYBASE_RELEASE`, redeploy, and run the live and
conformance test suites against the new database.

The integration tests apply migrations only when the startup check fails, or when `FLYBASE_TEST_MIGRATE=1`
asks for a rebuild. Rebuilding takes exclusive locks, so do not point a test run with that flag at a
database serving traffic.

## HTTP routes

| Route | Purpose | Exposure |
|---|---|---|
| `/mcp` | MCP Streamable HTTP endpoint | Public, behind nginx |
| `/health` | Liveness and readiness JSON | Loopback only |
| `/metrics` | Prometheus text | Loopback only |

The MCP service runs stateless: it issues no `Mcp-Session-Id`, keeps no per-client state, and answers each
JSON-RPC POST with `application/json` rather than an event stream. Any replica can serve any request, so no
sticky sessions are needed. Request bodies are capped at 1 MiB.

`/health` returns:

```json
{"status": "ok", "release": "FB2026_03", "database": "ok", "migrations": "ok",
 "pool": {"size": 4, "available": 3}, "blast": "unconfigured",
 "caches": {"response": {"entries": 12, "bytes": 48213}, "rnaseq_matrix": [{"dataset": "FlyAtlas2", "genes": 16608, "bytes": 3271000}], "blast_jobs": 0}}
```

- `database` is `ok` when a trivial query succeeds and `error` otherwise. When the database checks run past
  3 s, `database` reads `error` even if the trivial query itself succeeded.
- `migrations` is `ok` when every migration in the binary is recorded in `mcp.migration_log`, `missing: <names>`
  when some are not, `error` when the log cannot be read, and `timeout` when the database checks run past 3 s.
- `blast` is `unconfigured` (no `BLAST_BASE_URL`), `ok`, or `unreachable` (the BLAST API's health route did not
  answer success within 2 s).
- `status` is `ok` only when `database` and `migrations` are both `ok`, and `degraded` otherwise. BLAST
  reachability never changes `status`, because every other tool keeps working without it.
- `pool` reports the open and idle connections.
- `caches` reports the response cache's entries and weighted bytes (structured JSON is counted four times, so
  `bytes` is the figure the cache bound applies to, not the heap size), each loaded RNA-seq matrix, and the
  number of BLAST jobs whose tokens are held.

The database checks and the BLAST probe run concurrently, so the route answers within about 3 s even when a
backend hangs; the image's `HEALTHCHECK` allows 10 s. The route always answers 200, so check `status`.

`/metrics` carries `flybase_mcp_tool_calls_total` (labels `tool`, `outcome`, where outcome is `ok` or an
error code), the `flybase_mcp_tool_duration_seconds` histogram (label `tool`),
`flybase_mcp_cache_events_total` (labels `cache`, `event`, where event is `hit` or `miss`), and
`flybase_mcp_rnaseq_matrix_loads_total` (label `dataset`). The miss counter counts callers, not computations:
identical concurrent calls share one computation but each logs a miss.

## Caches

The response cache holds up to 256 MiB of tool results (`FLYBASE_MCP_RESPONSE_CACHE_MB`) for 6 h. It lives in
process memory and is emptied by a restart; a release bump restarts the process, so no result outlives its
release. Each RNA-seq matrix is loaded per dataset on first use (about 3 MB each) and kept for the life of the
process. Both are reported in `/health`.

Reloading chado under the same `FLYBASE_RELEASE` without a restart serves the old results for up to 6 h.
Restart the container after any database reload.

Plan container memory for the response cache bound plus roughly 200 MB: the five RNA-seq matrices (about
12 MB together), the BLAST token table, and the JSON copies made when a cached result is served. Do not set a
compose memory limit below that.

BLAST job tokens live in process memory for 24 h. A restart, or a second replica without sticky routing, makes
in-flight jobs read as `not_found` through the MCP tools, while their public result pages on flybase.org still
work. One replica is the supported deployment.

## nginx

- Proxy only `/mcp` to the container. Do not route `/health` or `/metrics` publicly; scrape them on loopback.
- Forward the original `Host` header (`proxy_set_header Host $host;`). The server rejects any `/mcp` request
  whose host is not the public URL's host, `localhost`, or `127.0.0.1`, as protection against DNS rebinding.
  A request that sends an `Origin` header must match the origin of `FLYBASE_MCP_PUBLIC_URL`; requests
  without one, as most MCP clients send, pass.
- Responses are plain JSON, so default proxy buffering is fine. Set `proxy_read_timeout` above 60 s to
  cover the slowest tools.

```nginx
location /mcp {
    proxy_pass http://127.0.0.1:7200;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_read_timeout 90s;
}
```

Per-client rate limits belong at the edge (WAF or CDN). Inside the server, global semaphores bound
concurrent database work regardless of how many clients connect; a call that cannot get a permit within 5 s
fails with a "busy" error the client may retry.

Known tuning step: a cold `map_features` or `cytosearch` query over a full 2 Mb window walks the whole range
on the featureloc index before filtering by type, and can take about 20 s (2 to 3 s warm) while it holds a
heavy permit. A few such calls at once can occupy every heavy permit. If load tests show heavy-permit
starvation, the lever is a derived located-feature table (arm, type, fmin, fmax, ...) indexed on
(arm, type, fmin), added in a later migration.

## Logs

Logs are JSON lines on stdout, one object per event, from `tracing-subscriber`:

```json
{"timestamp": "2026-10-11T14:02:11.512Z", "level": "INFO",
 "fields": {"message": "flybase-mcp listening", "bind": "0.0.0.0:7200", "release": "FB2026_03"},
 "target": "flybase_mcp::server"}
```

`fields` holds the message and the event's structured fields. Database errors log `tool`, `sqlstate`, and
`error` with the server's message; the client receives a generic error without SQL text. Pool and permit
timeouts log at `WARN` with `tool`. Migrations log each `migration` name and its `elapsed_ms`. The
connection URL is never logged with its password.

`rmcp` logs one `ERROR` line, "failed to send pending response during drain", each time a client disconnects
before its response is written. It is not a server fault; do not alert on it.
