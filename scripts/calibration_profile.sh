#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Create a versioned calibration profile (assets + diagnostics) from a .plc build.

Usage:
  scripts/calibration_profile.sh \
    --plc <file.plc> \
    --profile <name> \
    [--io-map <io_map.toml>] \
    [--analog-calibration <analog_calibration.toml>] \
    [--out-root <dir>] \
    [--force]

Outputs under <out-root>/<profile>/:
  - build_rp2040/generated_program.rs
  - build_rp2040/io_map.template.toml
  - build_rp2040/analog_contract.toml
  - build_rp2040/analog_calibration.template.toml
  - build_rp2040/build_meta.json
  - (optional) io_map.toml
  - (optional) analog_calibration.toml
  - profile_manifest.json
  - diagnostics.md
USAGE
}

PLC=""
PROFILE=""
IO_MAP=""
CAL=""
OUT_ROOT="out/calibration_profiles"
FORCE="0"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --plc) PLC="${2:-}"; shift 2;;
    --profile) PROFILE="${2:-}"; shift 2;;
    --io-map) IO_MAP="${2:-}"; shift 2;;
    --analog-calibration) CAL="${2:-}"; shift 2;;
    --out-root) OUT_ROOT="${2:-}"; shift 2;;
    --force) FORCE="1"; shift 1;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

if [[ -z "$PLC" || -z "$PROFILE" ]]; then
  echo "Missing required args: --plc/--profile" >&2
  usage
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$OUT_ROOT" = /* ]]; then
  OUT_ROOT_ABS="$OUT_ROOT"
else
  OUT_ROOT_ABS="$REPO_ROOT/$OUT_ROOT"
fi
PROFILE_DIR="$OUT_ROOT_ABS/$PROFILE"
BUILD_OUT="$PROFILE_DIR/build_rp2040"

if [[ -e "$PROFILE_DIR" ]]; then
  if [[ "$FORCE" == "1" ]]; then
    rm -rf "$PROFILE_DIR"
  else
    echo "Profile already exists: $PROFILE_DIR (use --force to overwrite)" >&2
    exit 1
  fi
fi

mkdir -p "$PROFILE_DIR"

BUILD_CMD=(cargo run --release --bin rust_plc -- build-rp2040 "$PLC" --out "$BUILD_OUT")
if [[ -n "$CAL" ]]; then
  BUILD_CMD+=(--analog-calibration "$CAL")
fi

echo "[1/3] build-rp2040 for profile '$PROFILE'"
(
  cd "$REPO_ROOT"
  "${BUILD_CMD[@]}"
)

if [[ -n "$IO_MAP" ]]; then
  cp "$IO_MAP" "$PROFILE_DIR/io_map.toml"
fi
if [[ -n "$CAL" ]]; then
  cp "$CAL" "$PROFILE_DIR/analog_calibration.toml"
fi

MANIFEST_JSON="$PROFILE_DIR/profile_manifest.json"
DIAG_MD="$PROFILE_DIR/diagnostics.md"

echo "[2/3] generate profile_manifest.json + diagnostics.md"
REPO_ROOT="$REPO_ROOT" PROFILE="$PROFILE" PLC="$PLC" IO_MAP="$IO_MAP" CAL="$CAL" \
PROFILE_DIR="$PROFILE_DIR" BUILD_OUT="$BUILD_OUT" MANIFEST_JSON="$MANIFEST_JSON" \
DIAG_MD="$DIAG_MD" python3 - <<'PY'
import datetime as dt
import hashlib
import json
import os
import pathlib

def sha256(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

repo_root = pathlib.Path(os.environ["REPO_ROOT"])
profile = os.environ["PROFILE"]
profile_dir = pathlib.Path(os.environ["PROFILE_DIR"])
build_out = pathlib.Path(os.environ["BUILD_OUT"])
plc_input = os.environ["PLC"]
io_map_input = os.environ["IO_MAP"]
cal_input = os.environ["CAL"]

meta = json.loads((build_out / "build_meta.json").read_text(encoding="utf-8"))

def parse_generated_analog_contract(text: str):
    # Minimal parser for the repo-generated analog_contract.toml:
    # - Sections look like: [analog_inputs.ai0] / [analog_outputs.ao0]
    # - Keys inside are simple scalars.
    cur = None
    out = {"analog_inputs": {}, "analog_outputs": {}}
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            sec = line[1:-1].strip()
            cur = None
            if sec.startswith("analog_inputs."):
                cur = ("analog_inputs", sec.split(".", 1)[1])
                out[cur[0]].setdefault(cur[1], {})
            elif sec.startswith("analog_outputs."):
                cur = ("analog_outputs", sec.split(".", 1)[1])
                out[cur[0]].setdefault(cur[1], {})
            continue
        if cur and "=" in line:
            k, v = [x.strip() for x in line.split("=", 1)]
            # Strip quotes for strings; parse numbers loosely.
            if v.startswith('"') and v.endswith('"'):
                val = v[1:-1]
            else:
                try:
                    if "." in v or "e" in v.lower():
                        val = float(v)
                    else:
                        val = int(v)
                except Exception:
                    val = v
            out[cur[0]][cur[1]][k] = val
    return out

contract_text = (build_out / "analog_contract.toml").read_text(encoding="utf-8")
contract = parse_generated_analog_contract(contract_text)
analog_inputs = contract.get("analog_inputs") or {}
analog_outputs = contract.get("analog_outputs") or {}

tracked_files = [
    build_out / "generated_program.rs",
    build_out / "io_map.template.toml",
    build_out / "analog_contract.toml",
    build_out / "analog_calibration.template.toml",
    build_out / "build_meta.json",
]
if io_map_input:
    tracked_files.append(profile_dir / "io_map.toml")
if cal_input:
    tracked_files.append(profile_dir / "analog_calibration.toml")

hashes = {}
for p in tracked_files:
    if p.exists():
        hashes[str(p.relative_to(profile_dir))] = sha256(p)

manifest = {
    "profile": profile,
    "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
    "plc_input": plc_input,
    "io_map_input": io_map_input or None,
    "analog_calibration_input": cal_input or None,
    "build_meta": meta,
    "analog_summary": {
        "ai_channels": len(analog_inputs),
        "ao_channels": len(analog_outputs),
        "ai_ids": sorted(analog_inputs.keys()),
        "ao_ids": sorted(analog_outputs.keys()),
    },
    "sha256": hashes,
}
(pathlib.Path(os.environ["MANIFEST_JSON"])).write_text(
    json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
    encoding="utf-8",
)

lines = []
lines.append(f"# Calibration Profile: {profile}")
lines.append("")
lines.append(f"- generated_at: {manifest['generated_at']}")
lines.append(f"- plc_input: `{plc_input}`")
if io_map_input:
    lines.append(f"- io_map_input: `{io_map_input}`")
if cal_input:
    lines.append(f"- analog_calibration_input: `{cal_input}`")
lines.append(f"- tool_version: `{meta.get('tool_version')}`")
lines.append(f"- plc_sha256: `{meta.get('plc_sha256')}`")
lines.append("")
lines.append("## Analog Input Channels")
if analog_inputs:
    for ch, cfg in sorted(analog_inputs.items()):
        lines.append(
            f"- `{ch}`: min={cfg.get('min')} max={cfg.get('max')} "
            f"scale={cfg.get('scale')} offset={cfg.get('offset')} unit={cfg.get('unit')}"
        )
else:
    lines.append("- (none)")
lines.append("")
lines.append("## Analog Output Channels")
if analog_outputs:
    for ch, cfg in sorted(analog_outputs.items()):
        lines.append(
            f"- `{ch}`: min={cfg.get('min')} max={cfg.get('max')} ramp_ms={cfg.get('ramp_ms')} "
            f"scale={cfg.get('scale')} offset={cfg.get('offset')} unit={cfg.get('unit')}"
        )
else:
    lines.append("- (none)")
lines.append("")
lines.append("## Integrity")
for name, digest in sorted(hashes.items()):
    lines.append(f"- `{name}`: `{digest}`")
lines.append("")
lines.append("## Use This Profile")
lines.append("```bash")
lines.append("RUST_PLC_GENERATED_PROGRAM_RS=build_rp2040/generated_program.rs \\")
if io_map_input:
    lines.append("RUST_PLC_IO_MAP_TOML=io_map.toml \\")
else:
    lines.append("RUST_PLC_IO_MAP_TOML=build_rp2040/io_map.template.toml \\")
lines.append("RUST_PLC_ANALOG_CONTRACT_TOML=build_rp2040/analog_contract.toml \\")
lines.append("cargo build -p board-rp2040 --target thumbv6m-none-eabi --release")
lines.append("```")

(pathlib.Path(os.environ["DIAG_MD"])).write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

echo "[3/3] done"
echo "profile_dir: $PROFILE_DIR"
echo "manifest:    $MANIFEST_JSON"
echo "diagnostics: $DIAG_MD"
