# blast-jobs

A minimal BLAST+ job runner behind the FlyBase MCP's internal BLAST API (the contract is in
[`docs/handoff-private-side.md`](../docs/handoff-private-side.md) §7; the MCP's client is
`mcp/src/backends/blast.rs`). It runs NCBI BLAST+ against a directory of BLAST databases described by a
catalogue file. Nothing site-specific is compiled in, so it is reusable by anyone with a BLAST database
directory and a catalogue.

This crate is a sibling of `mcp/`, not a workspace member: it has its own `Cargo.toml` and `Cargo.lock`.

## Toolchain

Stable Rust with `clippy` and `rustfmt`. If cargo is not on your `PATH`:

    export PATH="$HOME/.cargo/bin:$PATH"

Run every command below from `blast-jobs/`.

    cargo test                                   # all targets
    cargo fmt && cargo clippy --all-targets -- -D warnings
    cargo deny check advisories licenses

## Configuration

One JSON file, passed as the first argument (`blast-jobs <config.json>`). Unknown keys are refused.

| Key | Default | Meaning |
|---|---|---|
| `bind` | required | listen address, e.g. `0.0.0.0:8800` |
| `blast_bin_dir` | required | directory holding `blastn blastp blastx tblastn tblastx` |
| `database_root` | required | root the catalogue prefixes are relative to |
| `catalogue` | required | catalogue file (below) |
| `database_release` | required | label returned in every `202` and shown to users |
| `state_root` | required | writable directory for job state |
| `concurrency` | 2 | searches run at once (at least 1) |
| `threads_per_job` | 2 | `-num_threads` per search |
| `max_queued` | 20 | queued jobs before `429 queue_full` |
| `wall_seconds` | 600 | wall-clock limit for blastn and blastp |
| `translated_wall_seconds` | 300 | wall-clock limit for blastx, tblastn and tblastx |
| `retention_seconds` | 86400 | how long a job and its results are kept |
| `max_results_bytes` | 16777216 | cap on the served `results.tsv`; at most 16 MiB, the MCP's limit |
| `default_evalue` | 10 | used when a request has no `evalue` |
| `default_max_hits` | 100 | used when a request has no `max_hits` |
| `max_hsps_per_subject` | 50 | `-max_hsps`, and the ceiling for an `advanced` override |
| `max_residues` | blastn 100000, blastp 100000, blastx 30000, tblastn 30000, tblastx 10000 | query residue cap per program; if given, all five programs must be listed |

## Catalogue

Any JSON file with a `databases` array whose entries carry `species`, `category`, `protein` and `prefix`
(other fields are ignored):

```json
{ "databases": [
  { "species": "dmel", "category": "assembly", "protein": false, "prefix": "dmel/dmel-assembly" }
] }
```

A request's `database` is the `category`; its `species` are the `species` codes. `prefix` is relative to
`database_root` and names a BLAST volume (`<prefix>.nsq` for nucleotide, `<prefix>.psq` for protein). At
startup every prefix must exist and contain no whitespace or quotes, no `(species, category)` pair may
repeat, and a category must have one molecule type; otherwise the service refuses to start. A search over
several species passes all their prefixes as one `-db` list, so its E-values are computed against the
combined database length.

## Validation

Every submit is checked independently of the MCP: program, database and molecule type, species (1 to 20
lowercase codes, deduplicated), one FASTA record within the program's residue cap and alphabet, `evalue`
in (0, 1000], `max_hits` 1 to 1000, `matrix` (not with blastn), `megablast` (blastn only), `genetic_code`
in the NCBI set (1-6, 9-16, 21-31, 33). The `advanced` string is checked against a typed per-program
allow-list of BLAST+ flags (`src/advanced.rs`, `TABLE`); everything else, including the flags the service
sets itself and anything that could reach the filesystem or network, is refused with `invalid_option`.
