# blast-jobs benchmark harness

`run.sh` loads a running `blast-jobs` service through its internal job API and records how long every
search takes. It sizes `concurrency`, `threads_per_job`, the container's CPU limit and the two wall limits
before a deployment. It is plain bash, `curl` and `jq` (bash 4.3+, curl 7.55+, jq 1.6+). It holds no
hostnames or secrets: the service URL and label are arguments, and the bearer token comes from the
environment.

## Usage

    export BLAST_JOBS_TOKEN=...           # the service's bearer token
    bench/run.sh <base-url> <label> [options]

    bench/run.sh http://127.0.0.1:8800 c2 --concurrency-hint 2 \
        --state-dir /path/to/state_root --poll-interval 1 \
        --probe-url http://127.0.0.1:8080/ --probe-url http://127.0.0.1:8080/some/report \
        --probe-baseline 120

The script checks `/internal/health` and the token first. It then submits every query of the selected set
`--repeat` times (default 3) and keeps at most `--in-flight` jobs (default 16) submitted and unfinished.
Each job is polled until it ends and its `results.tsv` is downloaded. `run.sh --help` lists every option.

- **Polling** waits the submit answer's `poll_after_seconds` (5 or 10) between status requests.
  `--poll-interval 1` polls every second for finer client-side timings.
- **Per-job times** come from `<state_root>/jobs/<id>/job.json` with `--state-dir`, when the harness runs
  on the service host and can read the state directory. Without it they come from what polling saw. A job
  that went from queued to complete between two polls then has no run time, and the others are only as
  fine as the poll interval.
- **The in-flight bound** keeps the service queue below `max_queued` (default 20). A `429 queue_full` is
  still retried after `retry_after_seconds`, and the retries are counted.
- **`--probe-url`** GETs each URL every `--probe-interval` seconds (default 1) for the whole run, so the
  latency of a co-hosted site during the load is captured. `--probe-baseline S` probes for S seconds
  before the first submit. Each sample is tagged `baseline`, `load` or `after`.
- **Host captures** (`sar -u`, `sar -d`, `sar -q`, `/proc/pressure/io`, `/proc/pressure/cpu`) are not
  run by the script. It prints the exact commands at start, with paths in the output directory, and saves
  them to `host-commands.txt`. Start them in a second shell before the run and stop them after it.

## Output

Everything goes to `--out` (default `./bench-out/<label>-<UTC time>/`).

| File | Contents |
|---|---|
| `jobs.csv` | one row per job (columns below) |
| `jobs.jsonl` | the same rows as JSON lines |
| `probe.csv` | `label,phase,epoch,http_code,seconds,url`, one row per probe sample |
| `summary.txt` | the summary printed at the end |
| `meta.txt` | arguments, health before the run, start and finish times |
| `host-commands.txt` | the host capture commands |
| `results/` | each `results.tsv`, only with `--keep-results` |

`jobs.csv` columns are `label, rep, query, program, database, species, megablast, job_id, state, error,
submitted_at, submit_epoch, done_epoch, queue_wait_s, wall_s, wall_source, end_to_end_s, result_bytes,
truncated, submit_retries, concurrency_hint`.

- `wall_s` is start to finish of the search and `queue_wait_s` is submit to start. `wall_source` says where
  they came from: `job.json`, `observed` (polling) or `none`.
- `end_to_end_s` is the client's view, from the accepted submit to the poll that saw the job end.
- `truncated` is `true` when `results.tsv` carried `X-Blast-Jobs-Truncated: true`.
- `state` is the service's `complete`, `failed` or `cancelled`, or one of the harness's own states:
  `submit_error`, `lost`, `poll_error`, `timeout`, `results_error` or `harness_error`. `error` holds the
  message.

The summary line gives the job counts by outcome, the truncated count, the 429 retries and the makespan.
`makespan_s` runs from the first submit to the last observed end. `service_makespan_s`, given when every job
has a `job.json`, runs from the first `submitted_at_ms` to the last `finished_at_ms`. Per program it gives
the nearest-rank p50 and p95 of `wall_s` and `end_to_end_s`. Per probe URL and phase it gives the sample
count, p50, p95, maximum and error count.

## Query set

`queries/manifest.tsv` lists one job per line: name, FASTA file, program, database category, species, the
`megablast` flag (`-` leaves it to the service default) and the set. `--queries DIR` uses another
directory with its own `manifest.tsv`.

The `standard` set is the timed mix, eight jobs per pass, so the default three passes submit 24 jobs.
"12 species" means `dmel dsim dsec dyak dere dana dpse dper dwil dmoj dvir dgri`.

| Name | Query | Program | Database | Species |
|---|---|---|---|---|
| `blastn_exon` | 500 bp exon | blastn (`megablast: false`) | assembly | dmel |
| `megablast_exon` | 500 bp exon | blastn (`megablast: true`) | assembly | dmel |
| `blastn_cdna` | 2,024 nt cDNA | blastn (service default) | genbank | dmel |
| `blastp_protein` | 376 aa protein | blastp | translation | dmel |
| `tblastn_protein_12sp` | 376 aa protein | tblastn | assembly | 12 species |
| `tblastx_10kb_12sp` | 10 kb genomic | tblastx, at the 10,000 residue cap | assembly | 12 species |
| `blastx_30kb` | 30 kb genomic | blastx, at the 30,000 residue cap | translation | dmel |
| `megablast_100kb` | 100 kb genomic | blastn (`megablast: true`), maximum input | assembly | dmel |

The `uncapped` set is the single `tblastx_100kb_uncapped` job: 100 kb genomic, tblastx, assembly, dmel. It is
selected with `--uncapped` and runs once by default. The default caps reject it (`invalid_sequence`, tblastx
accepts at most 10,000 residues). Run it against a throwaway config that raises the tblastx cap and the
translated wall limit, then put the production config back:

    "max_residues": {"blastn": 100000, "blastp": 100000, "blastx": 30000,
                     "tblastn": 30000, "tblastx": 100000},
    "translated_wall_seconds": 1800

### Sequence origin

All query sequences are public NCBI RefSeq records for *Drosophila melanogaster*, fetched with E-utilities
`efetch` (FASTA). Each file's header line names its accession and coordinates. Total size is about 145 KB.

| File | Source |
|---|---|
| `exon_500bp.fa` | NT_033779.5 (chromosome arm 2L, release 6) 14,617,002 to 14,617,501: 500 bp inside an *Adh* coding exon, equal to bases 601 to 1,100 of NM_001032097.2 |
| `cdna_2kb.fa` | NM_001032097.2, *Adh* transcript variant H mRNA, 2,024 nt |
| `protein_376aa.fa` | NP_523670.2, even skipped protein, 376 aa |
| `genomic_10kb.fa` | NT_033779.5 14,611,001 to 14,621,000 (10 kb around *Adh*) |
| `genomic_30kb.fa` | NT_033779.5 14,601,001 to 14,631,000 (30 kb around *Adh*) |
| `genomic_100kb.fa` | NT_033779.5 14,566,001 to 14,666,000 (100 kb around *Adh*) |

## A sizing run

The design's plan (§3.11 of the blast-jobs design) in harness terms. Run it from the service host, with no
production client pointed at the service yet.

1. **Cold pass.** Start a fresh container, then run one pass right away against databases untouched
   since it started: `--repeat 1`, label `cold`. Do not drop the page cache on a shared host.
2. **Warm-up.** Run one more `--repeat 1` pass and discard it.
3. **Baseline probes.** Start the host captures, then give the first run `--probe-baseline 120` with
   `--probe-url` on the co-hosted site's front page and on one heavy page.
4. **Concurrency levels.** Restart the container at each `concurrency` and CPU limit, then run the default
   three passes labelled to match. The levels are 2, 4 and 6 with `--cpus` set to twice the concurrency,
   plus concurrency 2 with `threads_per_job` 4 and `--cpus 4`. Pass `--concurrency-hint` and
   `--state-dir` each time. Submit one of the site's own BLAST jobs by hand during each load window.
5. **Uncapped measurement.** Run `--uncapped` once against the throwaway config above.
6. **Pick the largest concurrency that kept the host healthy.** Canary p95 and the site job must stay
   within 10% of baseline. Host idle must stay above 50% and io pressure `some avg10` under 5. The capped
   tblastx job must finish under its wall limit.

## Dry run without BLAST

The service's fake BLAST+ programs (`tests/fake-blast/`) let the harness run locally. The fixture
catalogue only has dmel and dsim, so the 12-species jobs need a scratch catalogue with an empty
`<species>/<species>-assembly.nsq` per species, plus dmel `translation` and `genbank`.
`<state_root>/fake-blast.env` controls the fake, for example `FAKE_BLAST_SLEEP=3` or `FAKE_BLAST_BIG=1`
(20 MiB of output, so the truncation path shows).

    cargo build
    BLAST_JOBS_TOKEN=dry-run ./target/debug/blast-jobs serve /path/to/scratch/config.json &
    BLAST_JOBS_TOKEN=dry-run bench/run.sh http://127.0.0.1:18899 dry --concurrency-hint 2 \
        --state-dir /path/to/scratch/state --poll-interval 1
