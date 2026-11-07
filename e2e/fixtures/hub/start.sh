#!/bin/sh
# start.sh PORT DATA: start `enocone serve` for one flow, in the background.
#
# Needs BIN (the enocone binary), WEB (the fixture app, for its Postmark
# stand-in) and OWNER (the owner's email). Output goes to DATA/serve.log so
# the exec step doesn't wait on it; the server stops itself after 3 minutes
# in case the flow fails before stopping it. GH_CLIENT=<id> turns on
# GitHub sign-in against the fixture's GitHub stand-in.
set -eu
port=$1 data=$2
pidf="${TMPDIR:-/tmp}/enocone-e2e-hub-$port.pid"
if [ -f "$pidf" ]; then kill "$(cat "$pidf")" 2>/dev/null || true; sleep 0.2; fi
mkdir -p "$data"
ENOCONE_DATA="$data" ENOCONE_LISTEN="127.0.0.1:$port" ENOCONE_PUBLIC_URL="http://127.0.0.1:$port" \
ENOCONE_OWNER_EMAIL="$OWNER" ENOCONE_POSTMARK_URL="$WEB/postmark" ENOCONE_POSTMARK_TOKEN=fixture-postmark-token \
ENOCONE_GITHUB_CLIENT_ID="${GH_CLIENT:-}" ENOCONE_GITHUB_CLIENT_SECRET="${GH_CLIENT:+fixture-github-secret}" \
ENOCONE_GITHUB_URL="$WEB/github" ENOCONE_GITHUB_API_URL="$WEB/github/api" ENOCONE_OIDC_ISSUER="$WEB/oidc" \
  "$BIN" serve >>"$data/serve.log" 2>&1 &
pid=$!
echo "$pid" > "$pidf"
( sleep 180; kill "$pid" 2>/dev/null ) >/dev/null 2>&1 &
i=0
until curl -fsS "http://127.0.0.1:$port/healthz" >/dev/null 2>&1; do
  i=$((i + 1)); [ "$i" -gt 50 ] && { echo "enocone serve did not start:"; cat "$data/serve.log"; exit 1; }
  sleep 0.1
done
echo "enocone serve ready on port $port"
