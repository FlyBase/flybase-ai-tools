#!/bin/sh
# Builds the fixture BLAST databases the way the FB2023_04 archive was built (-parse_seqids, v5 volumes),
# laid out as catalogue.json expects: <out>/dmel/dmel-transcript.* and <out>/dmel/dmel-translation.*.
# usage: build-db.sh <dir holding makeblastdb> <fixture dir> <out dir>
set -eu
bin=$1 fixture=$2 out=$3
mkdir -p "$out/dmel"
cp "$fixture/catalogue.json" "$out/catalogue.json"
"$bin/makeblastdb" -in "$fixture/notch.fa" -dbtype nucl -parse_seqids -blastdb_version 5 \
  -title "Notch fixture transcripts" -out "$out/dmel/dmel-transcript" >/dev/null
"$bin/makeblastdb" -in "$fixture/notch-protein.fa" -dbtype prot -parse_seqids -blastdb_version 5 \
  -title "Notch fixture translations" -out "$out/dmel/dmel-translation" >/dev/null
