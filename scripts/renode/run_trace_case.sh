#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Run a predefined trace baseline case with Renode and print logs to stdout.

Usage:
  scripts/renode/run_trace_case.sh --case <case_name> [--renode-bin <path>]

Cases live in:
  examples/trace_baselines/<case_name>/renode_trace.resc
USAGE
}

CASE_NAME=""
RENODE_BIN_ARG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --case) CASE_NAME="${2:-}"; shift 2;;
    --renode-bin) RENODE_BIN_ARG="${2:-}"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

if [[ -z "$CASE_NAME" ]]; then
  echo "Missing --case <case_name>" >&2
  usage
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CASE_RESC="$REPO_ROOT/examples/trace_baselines/$CASE_NAME/renode_trace.resc"
if [[ ! -f "$CASE_RESC" ]]; then
  echo "Case script not found: $CASE_RESC" >&2
  exit 1
fi

RENODE_BIN="$RENODE_BIN_ARG"
if [[ -z "$RENODE_BIN" ]]; then
  if [[ -n "${RENODE_BIN:-}" && -x "${RENODE_BIN}" ]]; then
    :
  elif command -v renode >/dev/null 2>&1; then
    RENODE_BIN="$(command -v renode)"
  else
    RENODE_BIN="$($REPO_ROOT/scripts/renode/ensure_renode.sh)"
  fi
fi

"$RENODE_BIN" --disable-xwt --console -e "include @$CASE_RESC"
