#!/usr/bin/env bash
# Loads fixture/data into the compose Postgres. Fails loudly on any copy error.
set -euo pipefail
cd "$(dirname "$0")"
URL="postgres://flybase_mcp:flybase_mcp@127.0.0.1:5499/flybase"
docker compose up -d
for _ in $(seq 1 60); do psql "$URL" -Atc "select 1" >/dev/null 2>&1 && break; sleep 1; done
echo "schema (errors for unsupported objects are listed, then counted)"
psql "$URL" -q -f data/schema.sql 2> data/schema.err || true
echo "schema errors: $(grep -c ERROR data/schema.err || true) (see data/schema.err; extension and ownership errors are expected)"
psql "$URL" -v ON_ERROR_STOP=1 -Atc "alter database flybase set search_path = public, flybase, gene, gene_group, humanhealth, dataclass, dataclass_relationship"
for f in data/*.tsv; do
  base=$(basename "$f" .tsv)
  t=${base//__/.}
  [ "$base" = zz_feature_golden_path ] && t=feature
  psql "$URL" -v ON_ERROR_STOP=1 -q -c "set session_replication_role = replica" -c "\\copy $t from '$f'"
  echo "loaded $t from $base"
done
psql "$URL" -Atc "analyze"
echo "fixture ready at $URL"
