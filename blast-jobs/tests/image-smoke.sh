#!/bin/sh
# Smoke test for the built image (CI job blast-jobs-image; also run by hand). Builds the fixture
# databases with the image's own makeblastdb, runs `blast-jobs check`, then serves and runs one real
# blastn search end to end inside the container, and waits for the HEALTHCHECK to report healthy.
# usage: tests/image-smoke.sh <image>   (run from blast-jobs/)
set -eu
image=$1
here=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
name=blast-jobs-smoke-$$
cleanup() {
  docker rm -f "$name" >/dev/null 2>&1 || true
  # Job files belong to the image's uid 10002, so remove them as that user.
  docker run --rm -v "$work/state:/state" --entrypoint rm "$image" -rf /state/jobs >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT
mkdir -p "$work/db" "$work/state"
chmod 0777 "$work/state"   # the image runs as uid 10002

docker run --rm --user "$(id -u):$(id -g)" --entrypoint sh \
  -v "$here/fixture:/fixture:ro" -v "$work/db:/db" \
  "$image" /fixture/build-db.sh /opt/ncbi-blast/bin /fixture /db

cat > "$work/config.json" <<JSON
{"bind": "0.0.0.0:8800", "blast_bin_dir": "/opt/ncbi-blast/bin", "database_root": "/db",
 "catalogue": "/db/catalogue.json", "database_release": "FB2023_04", "state_root": "/state",
 "concurrency": 1, "threads_per_job": 1}
JSON
mounts="-v $work/db:/db:ro -v $work/state:/state -v $work/config.json:/etc/blast-jobs/config.json:ro"

# shellcheck disable=SC2086
docker run --rm $mounts "$image" check /etc/blast-jobs/config.json
echo "check: ok"

# shellcheck disable=SC2086
docker run -d --name "$name" --init --read-only --tmpfs /tmp:size=64m --network none \
  -e BLAST_JOBS_TOKEN=smoke-token $mounts "$image" >/dev/null
api() { docker exec "$name" curl -fsS -H 'authorization: Bearer smoke-token' "$@"; }
for _ in $(seq 1 50); do
  docker exec "$name" curl -fsS http://127.0.0.1:8800/internal/health >/dev/null 2>&1 && break
  sleep 0.2
done
docker exec "$name" curl -fsS http://127.0.0.1:8800/internal/health; echo

query=$(awk '/^>/{n++; next} n==1' "$here/fixture/notch.fa" | tr -d '\n' | cut -c2001-2400)
submit=$(api -H 'content-type: application/json' \
  -d "{\"program\":\"blastn\",\"database\":\"transcript\",\"species\":[\"dmel\"],\"sequence\":\">query\\n$query\\n\",\"client\":\"smoke\"}" \
  http://127.0.0.1:8800/internal/jobs)
echo "submit: $submit"
id=$(echo "$submit" | sed -n 's/.*"job_id":"\([A-Za-z0-9_-]*\)".*/\1/p')
[ -n "$id" ] || { echo "no job_id" >&2; exit 1; }
state=
for _ in $(seq 1 100); do
  state=$(api "http://127.0.0.1:8800/internal/jobs/$id" | sed -n 's/.*"state":"\([a-z]*\)".*/\1/p')
  [ "$state" = complete ] || [ "$state" = failed ] && break
  sleep 0.2
done
[ "$state" = complete ] || { echo "job ended $state" >&2; api "http://127.0.0.1:8800/internal/jobs/$id" >&2; exit 1; }
api "http://127.0.0.1:8800/internal/jobs/$id/results.tsv" > "$work/results.tsv"
head -c 200 "$work/results.tsv"; echo
head -n 1 "$work/results.tsv" | grep -q '^qseqid	sseqid	pident' || { echo "bad header" >&2; exit 1; }
sed -n 2p "$work/results.tsv" | grep -q '^query	fbdb\.dmel\.transcript\.5460	100\.000	400	.*ID=FBtr0070507;' \
  || { echo "expected N-RA as the top hit" >&2; exit 1; }
echo "search: ok"

for _ in $(seq 1 60); do
  health=$(docker inspect -f '{{.State.Health.Status}}' "$name")
  [ "$health" = healthy ] && break
  [ "$health" = unhealthy ] && { echo "HEALTHCHECK unhealthy" >&2; exit 1; }
  sleep 1
done
[ "$health" = healthy ] || { echo "HEALTHCHECK still $health" >&2; exit 1; }
echo "healthcheck: $health"
