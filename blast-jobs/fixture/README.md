# Real-BLAST fixture

Two tiny BLAST databases for `tests/real_blast.rs` and the CI image check. The sequences and deflines are
copied verbatim (trailing spaces trimmed) from the archived FB2023_04 BLAST set the service serves in
production, read with `blastdbcmd -outfmt %f`:

| File | Entries |
|---|---|
| `notch.fa` (nucleotide, category `transcript`) | `fbdb.dmel.transcript.5460` N-RA (FBtr0070507, 10,150 bp); `fbdb.dmel.transcript.1` snoRNA:M-RA (FBtr0070292) |
| `notch-protein.fa` (protein, category `translation`) | `fbdb.dmel.translation.529` N-PA (FBpp0070483, 2,703 aa); `fbdb.dmel.translation.1` Nep3-PA (FBpp0070000) |

The archive was built with `-parse_seqids`, so `sseqid` is the local id (`fbdb.dmel.transcript.5460`) and
the FlyBase id is in `stitle` (`... ID=FBtr0070507; name=N-RA; ...`). `build-db.sh` uses the same flags:

    fixture/build-db.sh <dir holding makeblastdb> fixture <out dir>

`catalogue.json` maps `dmel transcript` and `dmel translation` to the built volumes, in the same layout as
the archive (`dmel/dmel-transcript`, `dmel/dmel-translation`).
