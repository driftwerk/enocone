#!/usr/bin/env bash
# The GitHub Action's steps: `enocone-action.sh run` and `enocone-action.sh publish`.
#
# Inputs arrive as environment variables (INPUT_*), never pasted into shell
# code. Outputs go to $GITHUB_OUTPUT and the job summary to
# $GITHUB_STEP_SUMMARY. Run from the project folder.
set -u
out="${GITHUB_OUTPUT:-/dev/null}"
summary="${GITHUB_STEP_SUMMARY:-/dev/null}"

case "${1:-}" in
run)
  env_args=()
  if [ -n "${INPUT_ENV:-}" ]; then env_args=(--env "$INPUT_ENV"); fi
  # Extra arguments are split on spaces, like a command line (no globbing).
  set -f
  # shellcheck disable=SC2086
  enocone run "${env_args[@]}" --out enocone-report ${INPUT_ARGS:-} 2>&1 | tee enocone-run.log
  code=${PIPESTATUS[0]}
  set +f
  case $code in
    0) outcome=passed ;;
    1) outcome=failed ;;
    2) outcome=invalid ;;
    *) outcome=error ;;
  esac
  # The run's last line: "3 passed   1 failed   0 flaky   0 pending   0 skipped".
  counts=$(grep -E '^[0-9]+ passed +[0-9]+ failed' enocone-run.log | tail -1)
  passed=$(printf '%s' "$counts" | sed -E 's/^([0-9]+) passed.*/\1/')
  failed=$(printf '%s' "$counts" | sed -E 's/^[0-9]+ passed +([0-9]+) failed.*/\1/')
  rm -f enocone-run.log
  upload=false
  case "${INPUT_UPLOAD:-always}" in
    always) upload=true ;;
    on-failure) [ "$outcome" != passed ] && upload=true ;;
    never) ;;
    *) echo "::warning title=enocone::upload must be always, on-failure or never; uploading anyway." && upload=true ;;
  esac
  {
    echo "exit-code=$code"
    echo "outcome=$outcome"
    echo "passed=${passed:-0}"
    echo "failed=${failed:-0}"
    echo "report=${INPUT_PROJECT:-.}/enocone-report"
    echo "upload=$upload"
  } >> "$out"
  # `enocone run` writes the counts to the job summary itself.
  exit "$code"
  ;;
publish)
  if [ ! -f enocone-report/results.json ]; then
    echo "::notice title=enocone::Nothing to publish: the run didn't produce results.json."
    exit 0
  fi
  if [ -z "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" ] && [ -z "${ENOCONE_HUB_TOKEN:-}" ]; then
    echo "::notice title=enocone::Not publishing: this job can't get an OIDC token. Add 'permissions: id-token: write' (pull requests from forks never get one)."
    exit 0
  fi
  if url=$(enocone publish); then
    echo "url=$url" >> "$out"
    echo "::notice title=enocone::Published $url"
    echo "Published to [$url]($url)" >> "$summary"
  else
    echo "::warning title=enocone::Publishing failed; the report is still in the uploaded artifact."
  fi
  ;;
*)
  echo "usage: enocone-action.sh run|publish" >&2
  exit 64
  ;;
esac
