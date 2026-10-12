#!/usr/bin/env bash
# Parity check for convert_coordinates (spec 7, "Parity"): converts FlyBase's release 5 anchor insertion sites
# through a running server and compares them with FlyBase's release 6 anchor table.
# Usage: scripts/coordinate_parity.sh http://127.0.0.1:7200
# Needs curl and jq. Prints "agree N/1193" and every disagreement; exits 1 if any disagree.
set -euo pipefail
BASE="${1:?base URL of a running flybase-mcp, e.g. http://127.0.0.1:7200}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
curl -fsSL --max-time 60 -o "$TMP/r5.txt" https://s3ftp.flybase.org/flybase/associated_files/Anchors.Rel5.txt
curl -fsSL --max-time 60 -o "$TMP/r6.tsv" https://s3ftp.flybase.org/flybase/associated_files/Anchors.Rel6.tsv
# Rel5 has no header; Rel6 has one. Insertion names differ in encoding (&agr; vs alpha), so pair by line order.
cut -f3 "$TMP/r5.txt" > "$TMP/in.txt"
tail -n +2 "$TMP/r6.tsv" | cut -f3 > "$TMP/expected.txt"
n5=$(wc -l < "$TMP/in.txt"); n6=$(wc -l < "$TMP/expected.txt")
[ "$n5" -eq "$n6" ] || { echo "anchor files differ in length: $n5 vs $n6" >&2; exit 1; }
: > "$TMP/got.txt"
split -l 500 "$TMP/in.txt" "$TMP/batch."
for b in "$TMP"/batch.*; do
  jq -Rn '[inputs]' "$b" > "$TMP/coords.json"
  jq -n --slurpfile coords "$TMP/coords.json" \
    '{jsonrpc: "2.0", id: 1, method: "tools/call", params: {name: "convert_coordinates", arguments: {coordinates: $coords[0]}}}' > "$TMP/req.json"
  curl -fsS --max-time 120 -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
    --data-binary @"$TMP/req.json" "$BASE/mcp" \
    | jq -r '.result.structuredContent.conversions[] | if .status == "mapped" then "\(.to.chromosome):\(.to.start)" else "?" end' >> "$TMP/got.txt"
done
paste "$TMP/in.txt" "$TMP/expected.txt" "$TMP/got.txt" > "$TMP/all.tsv"
agree=$(awk -F'\t' '$2 == $3' "$TMP/all.tsv" | wc -l)
echo "agree $agree/$n5"
awk -F'\t' '$2 != $3 { print "DISAGREE input=" $1 " expected=" $2 " got=" $3 }' "$TMP/all.tsv"
[ "$agree" -eq "$n5" ]
