# blast-jobs: the BLAST service behind the MCP

`blast-jobs/` is a small Rust service that implements the MCP's internal BLAST job API
([handoff §7](handoff-private-side.md#7-blast-internal-job-api-ticket-for-the-blast-service)). It runs
NCBI BLAST+ 2.17.0 directly against a directory of BLAST databases described by a catalogue file. It keeps
jobs in memory with one directory per job on disk, and it has no database. It serves no public result
pages. The MCP's three BLAST tools (`blast_submit`, `blast_status`, `blast_results`) are its only intended
client. Nothing FlyBase-specific is compiled in: paths, the release label, limits and the species set
all come from the config file and the catalogue.

## API

| Route | Answers |
|---|---|
| `POST /internal/jobs` | `202 {job_id, state: "queued", queue_position, poll_after_seconds, database_release}`; `400 {error, message}` where `error` is `invalid_program`, `invalid_database`, `invalid_species`, `invalid_sequence`, `invalid_evalue`, `invalid_max_hits`, `invalid_matrix`, `invalid_megablast`, `invalid_genetic_code` (each naming the request field, which the MCP maps to the tool argument of the same name), `invalid_option` for `advanced` and `client`, or `invalid_request` for a malformed body; `413` over 1 MiB; `429 {error: "queue_full", retry_after_seconds: 30}` with `Retry-After`; `503` when the state directory cannot be written |
| `GET /internal/jobs/{id}` | `200 {job_id, state, queue_position?, error?}`, where `state` is `queued`, `running`, `complete` or `failed`; `404 {error: "unknown_job"}` |
| `GET /internal/jobs/{id}/results.tsv` | `200 text/tab-separated-values`, a header line and then BLAST's `-outfmt "6 std stitle"` rows; `X-Blast-Jobs-Truncated: true` when the 16 MiB cap cut the rows (at a row boundary); `409 {error: "not_complete", state}`; `404` |
| `GET /internal/health` | `200 {status: "ok", queued, running, database_release}` or `503 {status: "degraded", reason}`; `?liveness=1` skips the queue-full check |

`X-Blast-Jobs-Truncated: true` is the only sign that rows were dropped: the body is still a well-formed
table, so a client that ignores the header under-counts the hits.

Every route except health needs `Authorization: Bearer <BLAST_JOBS_TOKEN>`; the token is compared in
constant time. `result_url` and `downloads` are never sent, and `cancelled` is never produced. Job ids
are 22 random URL-safe characters. Jobs and results are kept for `retention_seconds` after submission.

`sseqid` is the BLAST database's own sequence id. The FB2023_04 archive was built with `-parse_seqids`, so
`sseqid` is a local id such as `fbdb.dmel.transcript.5460`. The FlyBase id is in `stitle`
(`... ID=FBtr0070507; name=N-RA; ...`), and the MCP takes `flybase_id` from there.

## Configuration

One JSON file, `/etc/blast-jobs/config.json` in the image. A sample is in
[`blast-jobs/deploy/config.sample.json`](../blast-jobs/deploy/config.sample.json). Unknown keys are refused.

| Key | Default | Meaning |
|---|---|---|
| `bind` | required | listen address; `0.0.0.0:8800` in the container (the image's health check probes port 8800) |
| `blast_bin_dir` | required | directory holding `blastn blastp blastx tblastn tblastx`; `/opt/ncbi-blast/bin` in the image |
| `database_root` | required | root that catalogue prefixes are relative to |
| `catalogue` | required | the catalogue file (below) |
| `database_release` | required | returned in every `202` and shown to users; `FB2023_04` for FlyBase |
| `state_root` | required | writable job state (`jobs/<id>/`) |
| `concurrency` | 2 | searches run at once |
| `threads_per_job` | 2 | `-num_threads` per search |
| `max_queued` | 20 | queued jobs before `429 queue_full` |
| `wall_seconds` | 600 | wall-clock limit for blastn and blastp |
| `translated_wall_seconds` | 300 | wall-clock limit for blastx, tblastn and tblastx |
| `retention_seconds` | 86400 | job and result lifetime (the MCP's token lifetime) |
| `max_results_bytes` | 16777216 | cap on the served `results.tsv`; at most 16 MiB, the MCP's limit |
| `default_evalue` | 10 | used when a request has no `evalue` |
| `default_max_hits` | 100 | `-max_target_seqs` when a request has no `max_hits` |
| `max_hsps_per_subject` | 50 | `-max_hsps`, and the ceiling for an `advanced` override |
| `max_residues` | see below | query residue cap per program; if given, all five programs must be listed |

The token comes from the environment, never the config: `BLAST_JOBS_TOKEN`. The service refuses to start
without it, unless `BLAST_JOBS_ALLOW_NO_AUTH=1` is set (for local runs only).

`blast-jobs check <config.json>` loads the config and the catalogue, checks every database volume and the
BLAST binaries, and exits 0 or 1 with the reason. `blast-jobs serve <config.json>` runs the service; it is
the image's default command.

## Catalogue

Any JSON file with a `databases` array. Each entry carries `species`, `category`, `protein` and `prefix`;
other fields are ignored, so the archive's own `catalogue.json` works as is.

```json
{ "databases": [
  { "species": "dmel", "category": "transcript", "protein": false, "prefix": "dmel/dmel-transcript" }
] }
```

A request's `database` is a `category` and its `species` are `species` codes. The service accepts every
species in the catalogue; the MCP's species list is a client-side restriction. `protein` must match the
program: blastp and blastx search protein databases, and the others search nucleotide ones. Every
requested species must have the category, otherwise the request is `invalid_species` naming the missing
ones. `prefix` is relative to `database_root` and names a BLAST volume (`<prefix>.nsq` or `.psq`).
At startup every prefix must exist, neither it nor `database_root` may contain whitespace or quotes
(a space separates the databases in `-db`), and no `(species, category)` pair
may repeat. Otherwise the service refuses to start.

**Multi-species E-values.** A search over several species passes all their prefixes as one `-db` list. Its
E-values are computed against the combined database length, so they are not comparable with a
one-species search.

## Searches

Each job runs `<blast_bin_dir>/<program>` with argv only (no shell), an empty environment plus
`NCBI_DONT_USE_NCBIRC=1 NCBI_DONT_USE_LOCAL_CONFIG=1 BLAST_USAGE_REPORT=false HOME=/tmp`, and these flags:
`-db`, `-query`, `-out`, `-outfmt "6 std stitle"`, `-evalue`, `-max_target_seqs`, `-max_hsps`,
`-num_threads`, the `-task` default, the masking default, `-matrix`, and the genetic code for translated
programs. `advanced` options then override these defaults; they are never appended twice.

- **Task.** blastn always gets an explicit `-task`: `megablast` when `megablast: true`, otherwise `blastn`.
  BLAST+'s own default for `blastn` is megablast. The other programs use BLAST+'s default task unless
  `advanced` sets one.
- **Masking defaults.** `-dust yes` for blastn and `-seg yes` for blastp, blastx, tblastn and tblastx. This
  matches the FlyBase site's "low-complexity filter on" default (BLAST+ itself defaults blastp to
  `-seg no`). `advanced` can turn either off (`-dust no`, `-seg no`).
- **Residue caps and wall limits.**

  | Program | Query residues | Wall limit |
  |---|---|---|
  | blastn | 100,000 | `wall_seconds` (600 s) |
  | blastp | 100,000 | `wall_seconds` (600 s) |
  | blastx | 30,000 | `translated_wall_seconds` (300 s) |
  | tblastn | 30,000 | `translated_wall_seconds` (300 s) |
  | tblastx | 10,000 | `translated_wall_seconds` (300 s) |

  A search that runs past its limit is killed and fails with "search exceeded N s; try a shorter query or
  a smaller database". A search killed by the container's memory cap fails with "search ran out of
  memory; try a shorter query or fewer species". Failure messages carry no paths.
- **Request fields.** `evalue` is in (0, 1000]. `max_hits` is 1 to 1000. `matrix` is one of `BLOSUM62
  BLOSUM45 BLOSUM80 PAM30 PAM70` and is refused for blastn. `megablast` is blastn only. `genetic_code` is
  used by blastx, tblastn and tblastx and must be in the NCBI set 1-6, 9-16, 21-31 and 33; blastn and
  blastp ignore it without checking it. A rejected field answers `invalid_<field>`. The sequence is one
  FASTA record in the program's alphabet.

### `advanced`

At most 200 characters of `[A-Za-z0-9 _.=-]`, written as `-flag value` or `-flag=value` pairs, with at most
8 pairs. Each flag must be in this table for the program:

| Flag | Value | Programs |
|---|---|---|
| `task` | blastn: `blastn blastn-short megablast dc-megablast`; blastp: `blastp blastp-short blastp-fast`; blastx: `blastx blastx-fast`; tblastn: `tblastn tblastn-fast` | blastn blastp blastx tblastn |
| `word_size` | integer >= 4 (blastn), >= 2 (others) | all |
| `gapopen`, `gapextend` | integer >= 0 | all but tblastx |
| `penalty` | integer <= 0 | blastn |
| `reward` | integer >= 0 | blastn |
| `dust` | `yes` / `no` | blastn |
| `seg` | `yes` / `no` | blastp blastx tblastn tblastx |
| `soft_masking` | `true` / `false` | all |
| `lcase_masking` | switch, no value; accepted but has no effect, because the service uppercases the query | all |
| `ungapped` | switch, no value | all but tblastx |
| `strand` | `both plus minus` | blastn blastx tblastx |
| `perc_identity` | number 0 to 100 | blastn |
| `qcov_hsp_perc` | number 0 to 100 | all |
| `culling_limit` | integer >= 0 | all |
| `max_hsps` | integer >= 1, clamped to `max_hsps_per_subject` | all |
| `threshold` | number >= 0 | blastp blastx tblastn |
| `comp_based_stats` | `0 1 2 3 D F T t f` | blastp blastx tblastn |
| `window_size` | integer >= 0 | all |

Integers must also fit a C `int` (-2147483648 to 2147483647), so an oversized value is refused rather
than failing the job. Anything else is `invalid_option`. That covers a flag outside the table or not valid for the program, a
repeated flag, a `--` token, a value starting with `-` (except a negative number for a numeric flag, as in
`-penalty -3`), and a value for a switch. The flags the service owns are refused too (`evalue
max_target_seqs matrix num_threads query_gencode db_gencode outfmt db query out`), as is anything that
reaches files or the network (`-remote`, `-subject`, `-import_search_strategy`, `-seqidlist`, ...). With
`megablast: true`, a `-task` other than `megablast` is refused.

## Image

`blast-jobs/Dockerfile` builds the binary with `--locked` on `rust:1.90-bookworm`. It downloads
`ncbi-blast-2.17.0+-x64-linux.tar.gz` from NCBI's versioned HTTPS path and checks it against a pinned
SHA-256. The runtime stage is `debian:bookworm-slim` with `ca-certificates curl libgomp1` and the seven
binaries `blastn blastp blastx tblastn tblastx blastdbcmd makeblastdb` in `/opt/ncbi-blast/bin`. The build
runs each binary's `-version`, so a missing library fails the build. The image runs as uid 10002, is
about 330 MB, and has a `HEALTHCHECK` on `/internal/health?liveness=1`, so a busy service is not
restarted.

    docker build -t blast-jobs:<commit> blast-jobs/
    blast-jobs/tests/image-smoke.sh blast-jobs:<commit>    # check + one real search inside the image

## Running it next to the MCP

[`blast-jobs/deploy/run.sample.sh`](../blast-jobs/deploy/run.sample.sh) has the full command. Its shape:

```
docker run -d --name blast-jobs --network <mcp network> --init \
  --cpus 4 --cpu-shares 512 --memory 4g --pids-limit 256 --read-only --tmpfs /tmp:size=256m \
  --restart unless-stopped --log-opt max-size=20m --log-opt max-file=3 \
  --mount type=bind,src=<database dir>,dst=/db,readonly \
  --mount type=bind,src=<state dir, owned by uid 10002>,dst=/state \
  --mount type=bind,src=<config.json>,dst=/etc/blast-jobs/config.json,readonly \
  --env-file <secrets env file holding BLAST_JOBS_TOKEN> \
  blast-jobs:<commit>
```

- `--init` reaps a BLAST process the service killed on timeout. `--cpus` should be at least
  `concurrency x threads_per_job`. `--cpu-shares 512` lets other services on the host win when it is busy.
- No port is published. The MCP reaches the service by container name on the shared network. For
  host-side checks, add `-p 127.0.0.1:<port>:8800`. The service has no slow-client timeout, so keep it
  unpublished: its only peers should be the MCP, whose client has its own timeouts.
- **Token pairing.** The MCP's `BLAST_SERVICE_TOKEN` must equal the service's `BLAST_JOBS_TOKEN`. Set
  `BLAST_BASE_URL=http://blast-jobs:8800` and `BLAST_SERVICE_TOKEN=<the same value>` in the MCP's
  environment and recreate the MCP container. Its `/health` then shows `blast: ok`.
- **`unreachable` versus a full queue.** The MCP's `/health` probes `/internal/health` without
  `?liveness=1`, so when the queue holds `max_queued` jobs it reports `blast: unreachable` even though the
  service is up. Submits get `429 queue_full` at that point, and the MCP tells callers to retry later.
- **Rollback.** Remove `BLAST_BASE_URL` from the MCP's environment and recreate the MCP container. Its
  `/health` then shows `blast: unconfigured`, and the BLAST tools answer `backend_unavailable`.

## Tests

From `blast-jobs/`:

    cargo test                                                     # unit, API and fake-BLAST tests
    BLAST_JOBS_REAL_BLAST=<BLAST+ bin dir> cargo test --test real_blast

`tests/real_blast.rs` skips itself, printing why, unless `BLAST_JOBS_REAL_BLAST` is set. When it runs, it
builds two tiny databases from `blast-jobs/fixture/` (N-RA and N-PA plus a decoy each, copied from the
FB2023_04 archive with the archive's `makeblastdb` flags). It runs blastn, blastp, blastx, tblastn and
tblastx through the API and checks the known Notch hit for each. It also checks that `-task blastn-short`
changes the scoring, and that the masking defaults hide Notch's low-complexity opa and polyglutamine
repeats until `advanced` turns them off. CI runs this test with the pinned NCBI tarball. It also builds
the image and runs `tests/image-smoke.sh` against it.
