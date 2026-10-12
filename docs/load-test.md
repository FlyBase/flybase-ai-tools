# Load test

`scripts/load_test.sh` measures server-side latency under concurrent load (spec 7, "Load").

## Method

The script runs two phases with [`oha`](https://github.com/hatoo/oha), each for a fixed duration (60 s here):

- **Phase A**: light tools only, 40 concurrent connections. `resolve`, `get_report`, `search` and `get_sequence` get 10 connections each.
- **Phase B**: the same 40 light connections, plus 16 heavy connections. `batch_download` and `entities_for_term` get 8 each, which oversubscribes the 6 heavy permits.

Each connection repeats one fixed JSON-RPC `tools/call` body from `scripts/load/bodies/`:

| body | call |
|---|---|
| `resolve.json` | `resolve` "Notch", species Dmel |
| `get_report.json` | `get_report` FBgn0004647, sections identity, location, go_summary, phenotype_summary |
| `search.json` | `search` "wingless", data class gene |
| `get_sequence.json` | `get_sequence` X:3134870..3136870 |
| `batch_download.json` | `batch_download` 6 genes, fields symbol, name, location, alleles, references |
| `entities_for_term.json` | `entities_for_term` GO:0007219, genes, limit 100 |

For each run the script reports p50, p95, p99 latency (seconds), requests per second, and three response columns. `completed` is oha's `summary.successRate`, the share of requests that got any HTTP response. It is not an HTTP success rate: oha 1.16.0 reports 1.0 when every response is HTTP 500. `2xx share` and `non-2xx` come from oha's `statusCodeDistribution`. They give the share of 2xx responses and the count of each other status code. After each table the script lists every run whose responses were not all 2xx. A JSON-RPC tool error still returns HTTP 200, so tool outcomes come from the delta of `flybase_mcp_tool_calls_total` in `/metrics`, not from status codes. The script also counts `pool checkout failed` and `no permit within` warnings in the server log (`SERVER_LOG=<file>`). That count is cumulative over the log, so the phase B count includes phase A. It also samples `/health` every 5 s and reports the minimum pool `available`. A failed sample is skipped. Pool connections are opened lazily, so `available` is idle connections among those opened so far, and 0 does not mean the pool is at its limit.

```bash
cd mcp && cargo build --release
FLYBASE_MCP_BIND=127.0.0.1:7202 FLYBASE_MCP_PUBLIC_URL=http://127.0.0.1:7202 \
  FLYBASE_RELEASE=FB2026_02 CHADO_URL=postgres://... RUST_LOG=warn \
  FLYBASE_MCP_RESPONSE_CACHE_MB=0 target/release/flybase-mcp serve > mcp.log 2>&1 &
SERVER_LOG=mcp.log ../scripts/load_test.sh http://127.0.0.1:7202 60s
```

The test was run twice. The first run disabled the response cache with `FLYBASE_MCP_RESPONSE_CACHE_MB=0`, so every call reached the database. The second run used the default cache.

### Environment

| item | value |
|---|---|
| date | 2026-10-12 |
| commit under test | 55d7c43 (server code; the script and this file were added in the next commit) |
| server | release build, default pool size 32, light permits 24, heavy permits 6 |
| database | full FB2026_02 chado restore, local Postgres 16 in Docker |
| machine | one workstation, Intel Core i7-12700H, 20 logical CPUs, 31 GB RAM |
| client | `oha 1.16.0` on the same machine |

**Limitation.** The client is one process on one IP, on the same host as the server and database. The per-IP edge rate limit is not exercised, and there is no network latency. The numbers bound server-side latency only.

## Acceptance criteria

- **Phase A**: every light tool has a 2xx share of 1.0 and p95 under 1.0 s, there are zero `backend_unavailable` outcomes, and there are no pool-wait timeouts.
- **Phase B**: every light tool still has a 2xx share of 1.0 and p95 under 1.0 s, light tools have zero `backend_unavailable` outcomes, and there are no pool-wait timeouts. Heavy tools may return `backend_unavailable` ("busy") after the 5 s permit wait. That is the designed load shedding.

## Results, cache disabled

**Recorded runs and the status columns.** Both runs below were recorded before the script had the `2xx share` and `non-2xx` columns. The raw oha output was not kept, so those cells are blank. Every request still reached its tool handler. For each run, the `flybase_mcp_tool_calls_total` delta matches req/s x 60 s within the rounding of req/s, and every outcome is `ok`. A non-2xx response never reaches a handler and records no tool call. So the 2xx criterion is met through the metrics, not through the status columns. Future runs fill both columns.

`FLYBASE_MCP_RESPONSE_CACHE_MB=0`, the database-bound case.

| criterion | result |
|---|---|
| Phase A light 2xx share 1.0 | **pass, via metrics**. Tool calls match req/s x 60 s within rounding, and every outcome is `ok`. |
| Phase A light p95 < 1.0 s | **pass**. The highest p95 is `get_report` at 0.384 s. |
| Phase A zero `backend_unavailable` | **pass**. All outcomes were `ok`. |
| Phase A no pool-wait timeouts | **pass**. The log count is 0. |
| Phase B light 2xx share 1.0 | **pass, via metrics**. The same check holds. |
| Phase B light p95 < 1.0 s | **pass**. The highest p95 is `get_report` at 0.573 s. |
| Phase B zero light `backend_unavailable` | **pass**. All outcomes were `ok`. |
| Phase B no pool-wait timeouts | **pass**. The log count is still 0. |

With 16 connections competing for 6 heavy permits, no heavy call waited past the 5 s permit limit, so no heavy call was shed. `entities_for_term` p95 of 5.67 s includes its time queued for a permit.


### Phase A: 40 light connections (60s)

| run | p50 s | p95 s | p99 s | req/s | completed | 2xx share | non-2xx |
|---|---|---|---|---|---|---|---|
| resolve | 0.16 | 0.216 | 0.242 | 61 | 1.0 | | |
| get_report | 0.298 | 0.384 | 0.436 | 33 | 1.0 | | |
| search | 0.06 | 0.086 | 0.096 | 165 | 1.0 | | |
| get_sequence | 0.172 | 0.239 | 0.268 | 56 | 1.0 | | |

Tool outcomes during the phase (from /metrics):

| tool | outcome | calls |
|---|---|---|
| get_report | ok | 1957 |
| get_sequence | ok | 3379 |
| resolve | ok | 3660 |
| search | ok | 9882 |

Minimum pool connections available: 0
Pool-wait and permit timeouts in the server log: count the lines matching 'pool checkout failed' or 'no permit within' in the server's stdout for this phase (SERVER_LOG=<file> when the server was started with its output redirected):
0

### Phase B: 16 heavy plus 40 light connections (60s)

| run | p50 s | p95 s | p99 s | req/s | completed | 2xx share | non-2xx |
|---|---|---|---|---|---|---|---|
| batch_download | 2.44 | 3.146 | 3.372 | 3 | 1.0 | | |
| entities_for_term | 4.296 | 5.666 | 5.738 | 2 | 1.0 | | |
| resolve_b | 0.216 | 0.289 | 0.318 | 46 | 1.0 | | |
| get_report_b | 0.4 | 0.573 | 0.648 | 24 | 1.0 | | |
| search_b | 0.077 | 0.111 | 0.127 | 128 | 1.0 | | |
| get_sequence_b | 0.235 | 0.315 | 0.364 | 42 | 1.0 | | |

Tool outcomes during the phase (from /metrics):

| tool | outcome | calls |
|---|---|---|
| batch_download | ok | 198 |
| entities_for_term | ok | 107 |
| get_report | ok | 1458 |
| get_sequence | ok | 2530 |
| resolve | ok | 2771 |
| search | ok | 7697 |

Minimum pool connections available: 1
Pool-wait and permit timeouts in the server log: count the lines matching 'pool checkout failed' or 'no permit within' in the server's stdout for this phase (SERVER_LOG=<file> when the server was started with its output redirected):
0

## Results, cache enabled

Default cache size. The six fixed bodies are answered from the cache after the first call on each connection. The cache recorded 56 misses and about 2.02 million hits. The 56 misses equal the 40 + 16 connections that opened with a cold call. These numbers measure the cache and transport path, not the database.

| criterion | result |
|---|---|
| Phase A light 2xx share 1.0 | **pass, via metrics**. Tool calls match req/s x 60 s within rounding, and every outcome is `ok`. |
| Phase A light p95 < 1.0 s | **pass**. Every p95 is 0.004 s. |
| Phase A zero `backend_unavailable` | **pass** |
| Phase A no pool-wait timeouts | **pass**. The log count is 0. |
| Phase B light 2xx share 1.0 | **pass, via metrics** |
| Phase B light p95 < 1.0 s | **pass**. Every p95 is 0.007 s. |
| Phase B zero light `backend_unavailable` | **pass** |
| Phase B no pool-wait timeouts | **pass**. The log count is 0. |


### Phase A: 40 light connections (60s)

| run | p50 s | p95 s | p99 s | req/s | completed | 2xx share | non-2xx |
|---|---|---|---|---|---|---|---|
| resolve | 0.002 | 0.004 | 0.006 | 4614 | 1.0 | | |
| get_report | 0.002 | 0.004 | 0.006 | 4215 | 1.0 | | |
| search | 0.002 | 0.004 | 0.006 | 4675 | 1.0 | | |
| get_sequence | 0.002 | 0.004 | 0.005 | 4893 | 1.0 | | |

Tool outcomes during the phase (from /metrics):

| tool | outcome | calls |
|---|---|---|
| get_report | ok | 252963 |
| get_sequence | ok | 293639 |
| resolve | ok | 276857 |
| search | ok | 280547 |

Minimum pool connections available: 1
Pool-wait and permit timeouts in the server log: count the lines matching 'pool checkout failed' or 'no permit within' in the server's stdout for this phase (SERVER_LOG=<file> when the server was started with its output redirected):
0

### Phase B: 16 heavy plus 40 light connections (60s)

| run | p50 s | p95 s | p99 s | req/s | completed | 2xx share | non-2xx |
|---|---|---|---|---|---|---|---|
| batch_download | 0.003 | 0.007 | 0.01 | 2210 | 1.0 | | |
| entities_for_term | 0.003 | 0.007 | 0.01 | 2034 | 1.0 | | |
| resolve_b | 0.003 | 0.007 | 0.01 | 2780 | 1.0 | | |
| get_report_b | 0.003 | 0.007 | 0.011 | 2605 | 1.0 | | |
| search_b | 0.003 | 0.007 | 0.01 | 2802 | 1.0 | | |
| get_sequence_b | 0.003 | 0.007 | 0.01 | 2908 | 1.0 | | |

Tool outcomes during the phase (from /metrics):

| tool | outcome | calls |
|---|---|---|
| batch_download | ok | 132606 |
| entities_for_term | ok | 122082 |
| get_report | ok | 156356 |
| get_sequence | ok | 174517 |
| resolve | ok | 166851 |
| search | ok | 168145 |

Minimum pool connections available: 4
Pool-wait and permit timeouts in the server log: count the lines matching 'pool checkout failed' or 'no permit within' in the server's stdout for this phase (SERVER_LOG=<file> when the server was started with its output redirected):
0

## Server log

There were no warnings. When each phase ends, `oha` drops the requests still in flight, and rmcp logs an `ERROR` line for each one: `failed to send pending response during drain` (or `fail to response message`) with `channel closed`. Without the cache there were 40 such lines at the end of phase A and 55 at the end of phase B, which matches the number of open connections. With the cache there were 6 in total. These lines come from clients disconnecting, not from server faults.
