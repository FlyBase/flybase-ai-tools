# Private-side handoff for the FlyBase MCP server

What the public repo cannot hold. This file states only what the server needs from its host; every
operational choice is marked "decide in the private repo" and recorded there, never here. The server itself
is documented in `deployment.md`; the tool surface in `tools.md`.

## 1. The `chado-mcp` server

`chado-mcp` is a dedicated PostgreSQL 16 server (`<chado-mcp-host>` in `CHADO_URL`). Each release is a
database on it restored from the same release dump as public chado. It is reachable only from the MCP
container and the hosts that run migrations and tests. For sizing: the FB2026_02 development restore
holds 98 million `feature` rows and the `mcp` schema adds the name index, CV closure, band coordinates,
RNA-seq expression and matrix tables, the DIOPT ortholog table, and trigram indexes on top; keep room for two
releases side by side during a load (section 3). The server sets its own statement timeouts per session;
leave the server default.

- [ ] Instance, storage, and PostgreSQL settings: decide in the private repo.
- [ ] Network reachability (MCP container and ops hosts only): decide in the private repo.
- [ ] Owner role for restores and `flybase-mcp migrate`: decide in the private repo.
- [ ] Backup policy (the database is rebuildable from the release dump): decide in the private repo.

## 2. The runtime role

```sql
create role flybase_mcp login password '<secret>' connection limit 40;
```
`007_grants` gives it `usage` and `select` on every schema the queries read and `execute` on the helper
functions. It owns nothing and has no write grant. Rotate the password by changing `CHADO_URL` and restarting
the container.

- [ ] Role created once per cluster; password held with the other deployment secrets.

## 3. Release runbook step (after the public chado load)

1. Restore the release dump into a new database on the `chado-mcp` server (for example `flybase_<release>`) as
   the owner role, so the previous release keeps serving until the switch.
2. Download `precomputed_files/orthologs/dmel_human_orthologs_disease_fb_<release>.tsv.gz` from the release
   FTP directory.
3. Run `flybase-mcp migrate --diopt <file>` against the new database with `CHADO_URL` set to the owner role
   (same image as `serve`; a few minutes). `migrate` loads the full server configuration and records
   `FLYBASE_RELEASE` in `mcp.migration_log`, so set `FLYBASE_RELEASE` to the new release value (and
   `FLYBASE_MCP_PUBLIC_URL`) before it runs; the `migrate` example in `deployment.md` shows the variables.
4. Set `CHADO_URL` to the new database with the runtime role and `FLYBASE_RELEASE=FB<year>_<n>` in the compose
   environment, then redeploy `flybase-mcp`. The server refuses to start if a migration is missing.
5. Check `GET /health` on loopback: `status: ok`, `migrations: ok`, `release` equal to the new value.
6. Run the read-only test binaries against the new database with the runtime role, from an ops host that
   can reach `<chado-mcp-host>`:

   ```bash
   git clone https://github.com/FlyBase/flybase-ai-tools && cd flybase-ai-tools
   git checkout <deployed commit>          # the commit the running image was built from
   cd mcp                                  # cargo runs from the crate directory
   export PATH="$HOME/.cargo/bin:$PATH"    # rustup toolchain; mcp/rust-toolchain.toml pins the version
   CHADO_URL='postgres://flybase_mcp:<secret>@<chado-mcp-host>:5432/flybase_<release>' \
     cargo test --test cache --test conformance --test live --test tools_batch --test tools_blast \
       --test tools_coordinates --test tools_entities --test tools_homepage --test tools_interactions \
       --test tools_map --test tools_orthologs --test tools_report_aberration --test tools_report_gene \
       --test tools_report_group --test tools_report_insertion --test tools_report_other \
       --test tools_report_record --test tools_report_transcript --test tools_resolve --test tools_rnaseq \
       --test tools_search --test tools_sequence --test tools_validate --test tools_vocabulary \
       --no-fail-fast -- --test-threads=2
   ```

   That is every test binary except `migrations`. **Never run the `migrations` test binary (or plain
   `cargo test` / `cargo test --tests`) against a serving database:** it rebuilds the derived tables under
   exclusive locks, empties `mcp.diopt_orthologs`, and rewrites the release recorded in `mcp.migration_log`.
   Never set `FLYBASE_TEST_MIGRATE`. Do set `FLYBASE_TEST_FULL_DB=1`: the harness otherwise recognises a
   full database only by the development port, and without the flag it runs the small-fixture branch of each
   test (which expects seeded rows on the first page and no annotation cap) against your full restore and
   fails. The flag only selects expectations; it writes nothing. Add `--no-fail-fast` so every binary runs.
   Some full-database assertions pin exact counts from the release the tests were written against; a count
   that moved with the release is data drift, not a server fault, and is listed in the repo's test notes.
   `CHADO_URL` and `FLYBASE_TEST_FULL_DB` are the only variables the tests read from the environment;
   the harness sets its own bind address, public URL, and release label (`FB_TEST`), so the deployment's
   `FLYBASE_RELEASE` is not needed here. Using the runtime role means a test cannot write even if its
   startup check fails, and `--test-threads=2` keeps the run to a few connections (each test server opens a
   pool of 4) under the role's connection limit while the production pool is open. Then run
   `scripts/coordinate_parity.sh http://127.0.0.1:7200` on the container host (it also fetches FlyBase's
   public anchor tables).
7. Drop the previous release's database once the new one has served a day.
8. Run `scripts/load_test.sh` against staging before the first production cutover and after any change to
   pool or permit sizes. It reads `/health` and `/metrics`, so point it at the loopback port.

- [ ] Steps 1 to 7 added to the release runbook after the public chado step.

## 4. Container and compose service

The public `deploy/docker-compose.sample.yml` is the template. Private values: the image tag, `CHADO_URL`,
`BLAST_BASE_URL` and `BLAST_SERVICE_TOKEN` (section 7), `CHADO_POOL_SIZE`, `FLYBASE_MCP_PUBLIC_URL`. Publish
port 7200 on loopback only. Run one replica: BLAST job tokens live in process memory for 24 h, so a second
replica would need sticky routing.

- [ ] Service in the production compose stack with `restart: unless-stopped` and the image's `HEALTHCHECK`.
- [ ] Secrets injected at deploy; none in any repo.

## 5. Reverse proxy

Proxy only `/mcp` to the container and forward the original `Host` header; the server rejects any `/mcp`
request whose host is not the public URL's host. Cap request bodies at 1 MiB (the server also does) and allow
at least 90 s for the slowest tools. Do not expose `/health` or `/metrics`. A worked nginx block is in
`deployment.md`.

- [ ] Server block for the public hostname in place; `/health` returns 404 from outside.

## 6. CDN and edge

Requirements the server imposes on the CloudFront distribution in front of the proxy: the origin request
policy must forward the `Host` header and all viewer headers (the server checks `Host` and, when present,
`Origin`; MCP clients send `Accept`, `Content-Type`, `MCP-Protocol-Version`, and JSON bodies); caching must be
disabled for `/mcp` (every call is a POST with a JSON-RPC body; GET and DELETE are part of the transport);
allowed methods must include GET, HEAD, OPTIONS, POST, and DELETE. The public hostname needs a DNS record and
a certificate.

- [ ] Distribution, DNS record, certificate: decide in the private repo.
- [ ] Attach the site's web ACL and an MCP-specific rule group (per-IP rate limit, body-size handling for
      100 kB BLAST sequences and 500-ID batches, method restriction): thresholds and rule tuning decided in
      the private repo.
- [ ] Any bot-protection or challenge tooling that could affect non-browser clients: decide in the private repo.

## 7. BLAST internal job API (ticket for the BLAST service)

The MCP is a client of the BLAST service over the private network with a bearer token. Add these routes
to the BLAST service; the MCP's client (`mcp/src/backends/blast.rs`) and its tests are written to this
contract.

`BLAST_BASE_URL` is the service root and must be `http://` (the MCP has no TLS backend and rejects an
`https://` value at startup); a base with a path is treated as a directory. The MCP never follows redirects:
any 3xx is a service fault. Timeouts on the MCP side: 3 s connect, 15 s per request, 5 s for a status poll,
2 s for the health probe.

| Route | Request | Responses |
|---|---|---|
| `POST /internal/jobs` | JSON `{program, database, species[], sequence, evalue?, megablast?, max_hits?, matrix?, genetic_code?, advanced?, client}`; `program` in `blastn blastp blastx tblastn tblastx`; `database` in `assembly transcript translation intergenic syntenic_region transposon canonical_transposons genbank est gss genpept`; `species` lowercase abbreviations, deduplicated (`dmel dsim dsec dyak dere dana dpse dper dwil dmoj dvir dgri`); `sequence` one FASTA record headed `>query`, uppercase residues, up to 100,000 residues; `evalue` in (0, 1000]; `max_hits` 1 to 1000; `matrix` one of `BLOSUM62 BLOSUM45 BLOSUM80 PAM30 PAM70`; `genetic_code` 1 to 33; `advanced` at most 200 characters of `[A-Za-z0-9 _.=-]` (letters, digits, space, underscore, dot, equals, hyphen; the same set `mcp/src/tools/blast.rs` checks); optional keys are omitted when unset; `client` is `flybase-mcp` | `202 {job_id, state: "queued", queue_position, poll_after_seconds, database_release}`; `400` (or `422`) `{error: invalid_sequence \| invalid_program \| invalid_database \| invalid_species \| invalid_option, message}`; `429 {error: "queue_full", retry_after_seconds}`; `401` or `403`; `503` |
| `GET /internal/jobs/{job_id}` | none | `200 {job_id, state: queued \| running \| complete \| failed \| cancelled, queue_position, error, result_url, downloads: {tsv, xml, txt}}` (`result_url` and `downloads` once complete, pointing at public flybase.org pages; the MCP passes on only URLs that start with `https://flybase.org/` and drops any other `result_url` or `downloads` map); `404 {error: "unknown_job"}` |
| `GET /internal/jobs/{job_id}/results.tsv` | none | `200` with `Content-Type` `text/tab-separated-values` or `text/plain` (any charset), header `qseqid sseqid pident length mismatch gapopen qstart qend sstart send evalue bitscore stitle` (BLAST `-outfmt "6 std stitle"`, tab-separated), one row per HSP in BLAST order; `409 {error: "not_complete", state}`; `404` |
| `GET /internal/health` | none, no auth | `200` when jobs can be accepted, else `503` |

All routes except `/internal/health` require `Authorization: Bearer <BLAST_SERVICE_TOKEN>`. Job IDs are
opaque strings of 8 to 64 URL-safe characters (`[A-Za-z0-9_-]`); any other `job_id` in a `202` is treated as a
service fault. Jobs and their results are retained for 24 h after submission (the MCP's token lifetime). The
MCP holds at most two concurrent submits, reads at most 16 MiB of `results.tsv`, and never polls on a
client's behalf; clients poll `blast_status` at the returned interval. The `database_release` string (for
example `FB2023_04 (Dmel r6.53)`) is shown to users, so keep it accurate. The `error` text of a failed job is
also shown to users verbatim, so it must carry no paths or hostnames. The service never sees the MCP's
per-job client tokens.

`blast-jobs/` in this repository implements these routes ([docs/blast-jobs.md](blast-jobs.md)): NCBI BLAST+
2.17.0 in its own container, run against a read-only BLAST database directory. It omits `result_url` and
`downloads` and never produces `cancelled`. Its `BLAST_JOBS_TOKEN` is the MCP's `BLAST_SERVICE_TOKEN`.

- [x] Routes implemented (`blast-jobs/`, reference implementation).
- [ ] Service deployed next to the MCP; the base URL and token recorded with the deployment secrets.
- [ ] `BLAST_BASE_URL` and `BLAST_SERVICE_TOKEN` added to the compose service; `/health` shows `blast: ok`.

## 8. Monitoring and secrets

`/metrics` (loopback) carries tool calls by outcome, latency histograms, cache events, and matrix loads;
`/health` carries `status`, `migrations`, `blast`, pool, and cache sizes. Logs are JSON lines per tool call
(`tool`, `outcome`, `duration_ms`); at the default `info` level no sequences or secrets are logged (at
`debug`, rmcp logs tool arguments, including BLAST job tokens). Secrets: the database password and the BLAST
token.

- [ ] Scrape target, alert rules and thresholds: decide in the private repo.
- [ ] Log shipping for the container: decide in the private repo.
