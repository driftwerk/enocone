#!/bin/sh
# stop.sh PORT: stop the server start.sh started on PORT.
pidf="${TMPDIR:-/tmp}/enocone-e2e-hub-$1.pid"
[ -f "$pidf" ] || exit 0
pid=$(cat "$pidf"); kill "$pid" 2>/dev/null || true
i=0; while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 50 ]; do i=$((i + 1)); sleep 0.1; done
rm -f "$pidf"; echo "stopped"
