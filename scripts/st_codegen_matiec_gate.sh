#!/usr/bin/env bash
# Generate ST from core examples and compile with MATIEC (iec2c).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="${1:-$REPO_ROOT/out/st_codegen_matiec}"

if [[ "$OUT_DIR" != /* ]]; then
  OUT_DIR="$REPO_ROOT/$OUT_DIR"
fi

mkdir -p "$OUT_DIR"

if ! command -v iec2c >/dev/null 2>&1; then
  echo "[ST-MATIEC] iec2c not found in PATH. Install MATIEC first." >&2
  exit 1
fi

resolve_iec2c_workdir() {
  local iec2c_bin
  local dir

  iec2c_bin="$(command -v iec2c)"
  dir="$(dirname "$iec2c_bin")"
  while [[ "$dir" != "/" ]]; do
    if [[ -f "$dir/lib/ieclib.txt" ]]; then
      echo "$dir"
      return 0
    fi
    dir="$(dirname "$dir")"
  done

  if [[ -f "$REPO_ROOT/vendor/matiec/lib/ieclib.txt" ]]; then
    echo "$REPO_ROOT/vendor/matiec"
    return 0
  fi

  return 1
}

IEC2C_WORKDIR="$(resolve_iec2c_workdir || true)"
if [[ -z "$IEC2C_WORKDIR" ]]; then
  echo "[ST-MATIEC] Cannot locate iec2c lib/ieclib.txt. Check MATIEC install." >&2
  exit 1
fi

generate_and_compile() {
  local plc_path="$1"
  local stem="$2"
  local st_file="$OUT_DIR/${stem}.st"

  echo "[ST-MATIEC] Generate ST: $plc_path -> $st_file"
  (
    cd "$REPO_ROOT"
    cargo run --release --bin rust_plc -- gen-st "$plc_path" --out "$st_file" --program-name Main
  )

  echo "[ST-MATIEC] Compile ST with iec2c: ${stem}.st"
  (
    cd "$IEC2C_WORKDIR"
    iec2c -T "$OUT_DIR" "$st_file"
  )
}

generate_and_compile "examples/two_cylinder.plc" "two_cylinder"
generate_and_compile "examples/assembly_station.plc" "assembly_station"

echo "[ST-MATIEC] OK - generated ST files compile with MATIEC"
