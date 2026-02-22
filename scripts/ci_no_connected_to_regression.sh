#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

resolve_base() {
  if [[ $# -ge 1 && -n "${1:-}" ]]; then
    echo "$1"
    return 0
  fi

  if [[ -n "${GITHUB_BASE_REF:-}" ]]; then
    local remote_ref="origin/${GITHUB_BASE_REF}"
    if ! git rev-parse --verify --quiet "$remote_ref" >/dev/null; then
      git fetch --no-tags --depth=1 origin "$GITHUB_BASE_REF":"$remote_ref"
    fi
    echo "$remote_ref"
    return 0
  fi

  if git rev-parse --verify --quiet HEAD^ >/dev/null; then
    echo "HEAD^"
    return 0
  fi

  echo ""
}

BASE_REF="$(resolve_base "${1:-}")"
if [[ -z "$BASE_REF" ]]; then
  echo "[connected-to-guard] no base ref found; skip check"
  exit 0
fi

MERGE_BASE="$(git merge-base "$BASE_REF" HEAD)"
PATTERN='connected_to[[:space:]]*:'

violations="$(
  git diff --unified=0 "$MERGE_BASE"..HEAD -- \
    | awk -v pattern="$PATTERN" '
        /^\+\+\+ b\// {
          path = substr($0, 7)
          next
        }
        /^@@/ { next }
        /^\+/ {
          if ($0 ~ /^\+\+\+/) {
            next
          }
          line = substr($0, 2)
          if (line ~ pattern) {
            printf "%s: %s\n", path, line
          }
        }
      '
)"

if [[ -n "$violations" ]]; then
  echo "[connected-to-guard] FAIL: found newly added legacy topology attributes"
  echo "$violations"
  echo "Use driven_by/reports_to/detects (run scripts/migrate_connected_to.py for bulk migration)."
  exit 2
fi

echo "[connected-to-guard] PASS"
