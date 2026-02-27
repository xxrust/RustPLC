#!/usr/bin/env python3
"""OpenPLC trace utilities.

Subcommands:
- normalize-modbus: convert a raw OpenPLC Modbus CSV capture into normalized JSONL.
- compare: compare normalized SIL/OpenPLC variable traces with tick tolerance.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple


@dataclass
class Sample:
    tick: int
    vars: Dict[str, Any]


def _read_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def _parse_bool(raw: Any) -> Optional[bool]:
    if isinstance(raw, bool):
        return raw
    if isinstance(raw, (int, float)):
        return bool(raw)
    text = str(raw).strip().lower()
    if text in {"1", "true", "on", "yes"}:
        return True
    if text in {"0", "false", "off", "no"}:
        return False
    return None


def _parse_number(raw: Any) -> Optional[float]:
    if isinstance(raw, (int, float)):
        return float(raw)
    text = str(raw).strip()
    try:
        return float(text)
    except ValueError:
        return None


def _read_jsonl_samples(path: Path) -> List[Sample]:
    out: List[Sample] = []
    with path.open("r", encoding="utf-8") as f:
        for line_no, raw in enumerate(f, 1):
            line = raw.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"invalid JSON at {path}:{line_no}: {exc.msg}")

            if not isinstance(row, dict):
                raise SystemExit(f"invalid row at {path}:{line_no}: expected object")
            if "tick" not in row:
                raise SystemExit(f"invalid row at {path}:{line_no}: missing tick")

            tick = int(row["tick"])
            values = row.get("vars", row)
            if not isinstance(values, dict):
                raise SystemExit(f"invalid row at {path}:{line_no}: vars must be object")

            out.append(Sample(tick=tick, vars=dict(values)))

    out.sort(key=lambda s: s.tick)
    return out


def _value_equal(a: Any, b: Any) -> bool:
    if isinstance(a, bool) or isinstance(b, bool):
        pa = _parse_bool(a)
        pb = _parse_bool(b)
        return pa is not None and pb is not None and pa == pb

    na = _parse_number(a)
    nb = _parse_number(b)
    if na is not None and nb is not None:
        return math.isclose(na, nb, rel_tol=0.0, abs_tol=1e-9)

    return str(a) == str(b)


def compare_traces(
    sil: List[Sample],
    openplc: List[Sample],
    variables: List[str],
    tick_tolerance: int,
) -> Dict[str, Any]:
    total = 0
    matched = 0
    mismatches: List[Dict[str, Any]] = []

    # Index OpenPLC samples by tick for quick nearby search.
    by_tick: Dict[int, List[Sample]] = {}
    for sample in openplc:
        by_tick.setdefault(sample.tick, []).append(sample)

    for sil_sample in sil:
        for var in variables:
            if var not in sil_sample.vars:
                total += 1
                mismatches.append(
                    {
                        "tick": sil_sample.tick,
                        "var": var,
                        "reason": "missing_in_sil",
                    }
                )
                continue

            total += 1
            sil_value = sil_sample.vars[var]
            found = False

            for openplc_tick in range(
                sil_sample.tick - tick_tolerance, sil_sample.tick + tick_tolerance + 1
            ):
                for candidate in by_tick.get(openplc_tick, []):
                    if var not in candidate.vars:
                        continue
                    if _value_equal(sil_value, candidate.vars[var]):
                        found = True
                        break
                if found:
                    break

            if found:
                matched += 1
            else:
                mismatches.append(
                    {
                        "tick": sil_sample.tick,
                        "var": var,
                        "sil": sil_value,
                        "reason": "no_match_within_tolerance",
                    }
                )

    pass_rate = 1.0 if total == 0 else matched / total
    return {
        "total_checks": total,
        "matched_checks": matched,
        "pass_rate": pass_rate,
        "tick_tolerance": tick_tolerance,
        "mismatches": mismatches[:200],
    }


def cmd_compare(args: argparse.Namespace) -> int:
    sil = _read_jsonl_samples(Path(args.sil))
    openplc = _read_jsonl_samples(Path(args.openplc))
    variables = [v.strip() for v in args.vars.split(",") if v.strip()]
    if not variables:
        raise SystemExit("--vars must include at least one variable name")

    report = compare_traces(
        sil=sil,
        openplc=openplc,
        variables=variables,
        tick_tolerance=args.tick_tolerance,
    )
    report["variables"] = variables
    report["min_pass_rate"] = args.min_pass_rate
    report["passed"] = report["pass_rate"] >= args.min_pass_rate

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8") as f:
        json.dump(report, f, indent=2, ensure_ascii=False)
        f.write("\n")

    status = "PASS" if report["passed"] else "FAIL"
    print(
        f"[{status}] pass_rate={report['pass_rate']:.4f}, "
        f"matched={report['matched_checks']}, total={report['total_checks']}, "
        f"tol=±{args.tick_tolerance} tick"
    )

    return 0 if report["passed"] else 1


def _read_mapping(path: Path) -> Dict[str, Dict[str, Any]]:
    raw = _read_json(path)
    if not isinstance(raw, dict):
        raise SystemExit(f"mapping must be an object: {path}")

    out: Dict[str, Dict[str, Any]] = {}
    for var, entry in raw.items():
        if not isinstance(entry, dict):
            raise SystemExit(f"mapping for {var} must be an object")
        source = str(entry.get("source", "")).strip()
        address = entry.get("address")
        value_type = str(entry.get("type", "")).strip().lower()
        if source not in {"coil", "holding_register"}:
            raise SystemExit(f"mapping[{var}].source must be coil|holding_register")
        if not isinstance(address, int):
            raise SystemExit(f"mapping[{var}].address must be integer")
        if value_type not in {"bool", "int", "real"}:
            raise SystemExit(f"mapping[{var}].type must be bool|int|real")
        out[var] = {"source": source, "address": address, "type": value_type}
    return out


def _column_candidates(source: str, address: int) -> List[str]:
    if source == "coil":
        return [f"coil_{address}", f"coil.{address}", str(address)]
    return [
        f"hr_{address}",
        f"holding_register_{address}",
        f"holding_register.{address}",
        str(address),
    ]


def _parse_value_by_type(raw: Any, value_type: str) -> Optional[Any]:
    if value_type == "bool":
        return _parse_bool(raw)
    if value_type == "int":
        number = _parse_number(raw)
        return None if number is None else int(round(number))
    number = _parse_number(raw)
    return None if number is None else float(number)


def _pick_timestamp_ms(row: Dict[str, str]) -> float:
    for key in ("timestamp_ms", "time_ms", "timestamp", "time"):
        if key in row and row[key] != "":
            try:
                return float(row[key])
            except ValueError:
                pass
    raise SystemExit("raw CSV must contain timestamp_ms/time_ms/timestamp/time column")


def cmd_normalize_modbus(args: argparse.Namespace) -> int:
    mapping = _read_mapping(Path(args.mapping))
    raw_path = Path(args.raw)
    out_path = Path(args.out)

    rows_out: List[Dict[str, Any]] = []
    with raw_path.open("r", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for row_no, row in enumerate(reader, 2):
            if row is None:
                continue
            timestamp_ms = _pick_timestamp_ms(row)
            tick = int(round(timestamp_ms / args.tick_ms))
            vars_row: Dict[str, Any] = {}

            for var, spec in mapping.items():
                candidates = _column_candidates(spec["source"], spec["address"])
                raw_value = None
                for key in candidates:
                    if key in row and row[key] != "":
                        raw_value = row[key]
                        break
                if raw_value is None:
                    continue

                parsed = _parse_value_by_type(raw_value, spec["type"])
                if parsed is None:
                    raise SystemExit(
                        f"cannot parse value for {var} at CSV line {row_no}: {raw_value!r}"
                    )
                vars_row[var] = parsed

            rows_out.append({"tick": tick, "vars": vars_row})

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8") as f:
        for row in rows_out:
            f.write(json.dumps(row, ensure_ascii=False))
            f.write("\n")

    print(f"normalized {len(rows_out)} rows -> {out_path}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="OpenPLC trace tooling")
    sub = parser.add_subparsers(dest="command", required=True)

    p_compare = sub.add_parser(
        "compare",
        help="Compare normalized SIL/OpenPLC traces and enforce pass-rate threshold",
    )
    p_compare.add_argument("--sil", required=True, help="Path to SIL normalized trace JSONL")
    p_compare.add_argument(
        "--openplc", required=True, help="Path to OpenPLC normalized trace JSONL"
    )
    p_compare.add_argument(
        "--vars",
        required=True,
        help="Comma-separated variables to compare, e.g. _state,valve_a,valve_b",
    )
    p_compare.add_argument(
        "--tick-tolerance",
        type=int,
        default=1,
        help="Allowed tick offset when matching samples (default: 1)",
    )
    p_compare.add_argument(
        "--min-pass-rate",
        type=float,
        default=0.95,
        help="Minimum accepted pass rate (default: 0.95)",
    )
    p_compare.add_argument("--out", required=True, help="Output JSON report path")
    p_compare.set_defaults(func=cmd_compare)

    p_norm = sub.add_parser(
        "normalize-modbus",
        help="Normalize raw OpenPLC Modbus CSV trace into JSONL (tick + vars)",
    )
    p_norm.add_argument("--raw", required=True, help="Raw OpenPLC Modbus CSV path")
    p_norm.add_argument(
        "--mapping",
        required=True,
        help="Variable mapping JSON (var -> source/address/type)",
    )
    p_norm.add_argument(
        "--tick-ms",
        type=float,
        required=True,
        help="Tick period in ms used to map timestamps to ticks",
    )
    p_norm.add_argument("--out", required=True, help="Output normalized JSONL path")
    p_norm.set_defaults(func=cmd_normalize_modbus)

    return parser


def main(argv: Optional[Iterable[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(list(argv) if argv is not None else None)
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
