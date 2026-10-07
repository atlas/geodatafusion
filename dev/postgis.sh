#!/usr/bin/env bash
# Manage the PostGIS container used as the oracle for the sqllogictest suite.
#
#   dev/postgis.sh start    # start (or reuse) the container and wait until it accepts queries
#   dev/postgis.sh stop     # remove the container
#   dev/postgis.sh psql     # open psql inside the container
#   dev/postgis.sh status
#
# The runner connects via POSTGIS_URL (default postgresql://postgres:postgres@localhost:54329/postgres).
# Keep POSTGIS_IMAGE in sync with the PostGIS version the docs examples were extracted from.
set -euo pipefail

NAME="${POSTGIS_CONTAINER:-geodatafusion-postgis}"
# Pinned by digest: PostGIS 3.6.4 with GEOS 3.14.1, the versions the parity tests are recorded
# with. Move it together with geos-src (see tests/sqllogictests/README.md).
IMAGE="${POSTGIS_IMAGE:-docker.io/postgis/postgis:18-3.6@sha256:7e00e8c3539fdd43f513b98806c8204714dcd09dea683c259e333d7690317119}"
PORT="${POSTGIS_PORT:-54329}"

if command -v docker >/dev/null 2>&1; then
    ENGINE=docker
elif command -v podman >/dev/null 2>&1; then
    ENGINE=podman
else
    echo "Neither docker nor podman found" >&2
    exit 1
fi

running() {
    [ "$($ENGINE inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null)" = "true" ]
}

wait_ready() {
    for _ in $(seq 1 60); do
        if $ENGINE exec "$NAME" psql -U postgres -tAc "SELECT postgis_full_version()" >/dev/null 2>&1; then
            return 0
        fi
        sleep 1
    done
    echo "PostGIS did not become ready in time" >&2
    $ENGINE logs "$NAME" | tail -20 >&2
    exit 1
}

case "${1:-start}" in
start)
    if running; then
        echo "$NAME already running"
    else
        $ENGINE rm -f "$NAME" >/dev/null 2>&1 || true
        $ENGINE run -d --name "$NAME" -p "127.0.0.1:$PORT:5432" \
            -e POSTGRES_PASSWORD=postgres "$IMAGE" >/dev/null
    fi
    wait_ready
    # The postgis/postgis image creates the extension in template1 and postgres via an init
    # script; make sure it is present regardless.
    $ENGINE exec "$NAME" psql -U postgres -qc "SET client_min_messages = warning; CREATE EXTENSION IF NOT EXISTS postgis" >/dev/null
    echo "PostGIS ready at postgresql://postgres:postgres@localhost:$PORT/postgres"
    $ENGINE exec "$NAME" psql -U postgres -tAc "SELECT postgis_lib_version()"
    ;;
stop)
    $ENGINE rm -f "$NAME" >/dev/null
    echo "removed $NAME"
    ;;
psql)
    shift
    exec $ENGINE exec -it "$NAME" psql -U postgres "$@"
    ;;
status)
    if running; then echo "running"; else echo "not running"; exit 1; fi
    ;;
*)
    echo "usage: $0 {start|stop|psql|status}" >&2
    exit 1
    ;;
esac
