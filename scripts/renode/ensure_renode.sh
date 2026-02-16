#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Ensure a runnable Renode binary exists and print its path.

Usage:
  scripts/renode/ensure_renode.sh [--out-dir <dir>]

Behavior:
  - If RENODE_BIN is set and executable, prints it and exits.
  - Else if <out-dir>/renode exists, prints it and exits.
  - Else downloads latest linux-portable-dotnet tarball from builds.renode.io,
    extracts it into <out-dir>, prints <out-dir>/renode.
USAGE
}

OUT_DIR="out/tools/renode"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out-dir) OUT_DIR="${2:-}"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [[ "$OUT_DIR" != /* ]]; then
  OUT_DIR="$REPO_ROOT/$OUT_DIR"
fi

if [[ -n "${RENODE_BIN:-}" && -x "${RENODE_BIN}" ]]; then
  echo "$RENODE_BIN"
  exit 0
fi

if [[ -x "$OUT_DIR/renode" ]]; then
  echo "$OUT_DIR/renode"
  exit 0
fi

mkdir -p "$OUT_DIR"
TARBALL="$OUT_DIR/renode-latest.linux-portable-dotnet.tar.gz"

python3 - <<'PY' "$TARBALL"
import pathlib
import re
import sys
import urllib.request

out = pathlib.Path(sys.argv[1])
html = urllib.request.urlopen('https://builds.renode.io/', timeout=30).read().decode('utf-8', 'ignore')
matches = re.findall(r'href="(renode-[^"]+\.linux-portable-dotnet\.tar\.gz)"', html)
candidates = [m for m in matches if 'latest' not in m]
if not candidates:
    raise SystemExit('failed to locate latest renode linux-portable-dotnet artifact')
name = candidates[0]
url = 'https://builds.renode.io/' + name
urllib.request.urlretrieve(url, out)
print(f'downloaded {url} -> {out}', file=sys.stderr)
PY

tar xf "$TARBALL" -C "$OUT_DIR" --strip-components=1

if [[ ! -x "$OUT_DIR/renode" ]]; then
  echo "Failed to prepare Renode at $OUT_DIR/renode" >&2
  exit 1
fi

echo "$OUT_DIR/renode"
