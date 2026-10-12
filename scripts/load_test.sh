#!/usr/bin/env bash
# Load test (spec 7): light tools alone, then light tools while heavy tools saturate their permits.
# Usage: scripts/load_test.sh http://127.0.0.1:7200 [duration, default 60s]
# Needs oha (cargo install oha), curl, jq. Prints Markdown; paste the tables into docs/load-test.md.
set -euo pipefail
BASE="${1:?base URL of a running flybase-mcp}"
DUR="${2:-60s}"
cd "$(dirname "$0")"
OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT
HDR=(-H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream')

run() { # name body concurrency
  oha -z "$DUR" -c "$3" --no-tui --output-format json -m POST "${HDR[@]}" -D "load/bodies/$2.json" "$BASE/mcp" > "$OUT/$1.json"
}
metrics() { # a fresh server has no tool-call series yet; an empty snapshot is valid, not an error
  curl -fsS "$BASE/metrics" | { grep '^flybase_mcp_tool_calls_total{' || true; } | sort > "$1"
}
pool_sampler() { # writes min available to $1 until killed; a failed sample is skipped, not counted as 0
  local min=999999
  while :; do
    if a=$(curl -fsS --max-time 4 "$BASE/health" | jq -e '.pool.available' 2>/dev/null); then
      [ "$a" -lt "$min" ] && min=$a
      echo "$min" > "$1"
    fi
    sleep 5
  done
}
report() { # phase name...
  local phase=$1; shift
  echo; echo "### $phase ($DUR)"; echo
  echo "| run | p50 s | p95 s | p99 s | req/s | HTTP success |"; echo "|---|---|---|---|---|---|"
  for n in "$@"; do
    jq -r --arg n "$n" '"| \($n) | \(.latencyPercentiles.p50 | . * 1000 | round / 1000) | \(.latencyPercentiles.p95 | . * 1000 | round / 1000) | \(.latencyPercentiles.p99 | . * 1000 | round / 1000) | \(.summary.requestsPerSec | round) | \(.summary.successRate) |"' "$OUT/$n.json"
  done
  echo; echo "Tool outcomes during the phase (from /metrics):"; echo
  echo '| tool | outcome | calls |'; echo '|---|---|---|'
  awk 'FILENAME == ARGV[1] { before[$1] = $2; next } # not NR == FNR: the before file is empty on a fresh server
       { n = $2 - before[$1]; if (n > 0) { match($1, /tool="[^"]*"/); t = substr($1, RSTART + 6, RLENGTH - 7);
         match($1, /outcome="[^"]*"/); o = substr($1, RSTART + 9, RLENGTH - 10); printf "| %s | %s | %d |\n", t, o, n } }' \
      "$OUT/before.txt" "$OUT/after.txt"
  echo; echo "Minimum pool connections available: $(cat "$OUT/pool.txt" 2>/dev/null || echo 'no sample')"
  echo "Pool-wait and permit timeouts in the server log: count the lines matching 'pool checkout failed' or 'no permit within' in the server's stdout for this phase (SERVER_LOG=<file> when the server was started with its output redirected):"
  if [ -n "${SERVER_LOG:-}" ]; then grep -c "pool checkout failed\|no permit within" "$SERVER_LOG" || true; else echo "SERVER_LOG not set"; fi
}

echo "# Load test against $BASE"
echo; echo "oha: $(oha --version)"; echo; curl -fsS "$BASE/health" | jq -c '{release, status, pool}'

metrics "$OUT/before.txt"; pool_sampler "$OUT/pool.txt" & SAMPLER=$!
PIDS=()
run resolve resolve 10 & PIDS+=($!); run get_report get_report 10 & PIDS+=($!)
run search search 10 & PIDS+=($!); run get_sequence get_sequence 10 & PIDS+=($!)
wait "${PIDS[@]}"
kill $SAMPLER; metrics "$OUT/after.txt"
report "Phase A: 40 light connections" resolve get_report search get_sequence

rm -f "$OUT/pool.txt"
metrics "$OUT/before.txt"; pool_sampler "$OUT/pool.txt" & SAMPLER=$!
PIDS=()
run batch_download batch_download 8 & PIDS+=($!); run entities_for_term entities_for_term 8 & PIDS+=($!)
run resolve_b resolve 10 & PIDS+=($!); run get_report_b get_report 10 & PIDS+=($!)
run search_b search 10 & PIDS+=($!); run get_sequence_b get_sequence 10 & PIDS+=($!)
wait "${PIDS[@]}"
kill $SAMPLER; metrics "$OUT/after.txt"
report "Phase B: 16 heavy plus 40 light connections" batch_download entities_for_term resolve_b get_report_b search_b get_sequence_b
