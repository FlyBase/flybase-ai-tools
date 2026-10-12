#!/bin/sh
# Sample run script for blast-jobs next to the FlyBase MCP. Every variable below is required:
#   IMAGE         the image tag, e.g. blast-jobs:<commit>
#   NETWORK       the private Docker network the MCP container is on
#   DATABASE_DIR  the BLAST database directory (catalogue.json and the volumes); mounted read-only
#   STATE_DIR     writable job state, owned by uid 10002
#   CONFIG        config.sample.json from this directory, edited for the host
#   SECRETS_ENV   an env file holding only BLAST_JOBS_TOKEN=<token>; the MCP's BLAST_SERVICE_TOKEN is the
#                 same value
# The MCP reaches the service as http://blast-jobs:8800; no port is published. For host-side checks add
# `-p 127.0.0.1:<port>:8800` (loopback only).
set -eu
: "${IMAGE:?}" "${NETWORK:?}" "${DATABASE_DIR:?}" "${STATE_DIR:?}" "${CONFIG:?}" "${SECRETS_ENV:?}"
docker rm -f blast-jobs 2>/dev/null || true
exec docker run -d --name blast-jobs --network "$NETWORK" --init \
  --cpus 4 --cpu-shares 512 --memory 4g --pids-limit 256 --read-only --tmpfs /tmp:size=256m \
  --restart unless-stopped --log-opt max-size=20m --log-opt max-file=3 \
  --mount "type=bind,src=$DATABASE_DIR,dst=/db,readonly" \
  --mount "type=bind,src=$STATE_DIR,dst=/state" \
  --mount "type=bind,src=$CONFIG,dst=/etc/blast-jobs/config.json,readonly" \
  --env-file "$SECRETS_ENV" \
  "$IMAGE"
