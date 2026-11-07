#!/bin/sh
# seed.sh PORT DATA ARGS…: set up a running hub as its owner, over HTTP.
#   project:SLUG:NAME[:REPO]     create a project
#   invite:SLUG:WHO:ROLE         invite an email, @login or org/team
#   token:EMAIL                  print a publish token for EMAIL
#   link:EMAIL                   print a one-time sign-in link for EMAIL
#   remove:SLUG:KIND:SUBJECT     remove a member (KIND is email, github or team)
# Needs BIN and OWNER, like start.sh.
set -eu
port=$1 data=$2; shift 2
url="http://127.0.0.1:$port"
link=$(ENOCONE_DATA="$data" ENOCONE_PUBLIC_URL="$url" "$BIN" serve --sign-in-link "$OWNER")
jar="$data/owner.cookies"
curl -fsS -c "$jar" -o /dev/null -H "Origin: $url" --data-urlencode "token=${link#*token=}" "$url/auth/email"
post() { curl -fsS -b "$jar" -o /dev/null -H "Origin: $url" "$@"; }
for a in "$@"; do
  IFS=: read -r kind x y z <<END
$a
END
  case $kind in
    project) post --data-urlencode "slug=$x" --data-urlencode "name=$y" --data-urlencode "repo=${z:-}" "$url/projects" ;;
    invite) post --data-urlencode "who=$y" --data-urlencode "role=$z" "$url/$x/members" ;;
    token) ENOCONE_DATA="$data" ENOCONE_PUBLIC_URL="$url" "$BIN" serve --new-token "$x" ;;
    link) ENOCONE_DATA="$data" ENOCONE_PUBLIC_URL="$url" "$BIN" serve --sign-in-link "$x" ;;
    remove) post --data-urlencode "kind=$y" --data-urlencode "subject=$z" "$url/$x/members/remove" ;;
  esac
done
echo "seeded"
