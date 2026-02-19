#!/usr/bin/env python3
"""Evaluate case-specific HIL assertions against trace events."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate HIL case assertions against board_trace.jsonl"
    )
    parser.add_argument("--spec", required=True, help="Path to assertion spec JSON")
    parser.add_argument("--trace", required=True, help="Path to trace JSONL")
    parser.add_argument("--out", required=True, help="Path to output report JSON")
    return parser.parse_args()


def load_json(path: Path) -> Dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8") as f:
            return json.load(f)
    except Exception as err:  # pragma: no cover - defensive error path
        raise SystemExit(f"failed to load JSON {path}: {err}") from err


def load_trace(path: Path) -> List[Dict[str, Any]]:
    events: List[Dict[str, Any]] = []
    try:
        with path.open("r", encoding="utf-8") as f:
            for line_no, raw in enumerate(f, start=1):
                line = raw.strip()
                if not line:
                    continue
                try:
                    row = json.loads(line)
                except json.JSONDecodeError as err:
                    raise SystemExit(
                        f"invalid trace JSON at {path}:{line_no}: {err.msg}"
                    ) from err
                required = ("tick", "task", "from_step", "to_step", "reason")
                missing = [key for key in required if key not in row]
                if missing:
                    raise SystemExit(
                        f"invalid trace row at {path}:{line_no}: missing {missing}"
                    )
                row["index"] = len(events)
                events.append(row)
    except FileNotFoundError:
        raise SystemExit(f"trace file not found: {path}")
    return events


def find_matching_event(
    events: List[Dict[str, Any]], step_expect: Dict[str, Any]
) -> Tuple[Optional[int], Optional[Dict[str, Any]]]:
    for index, event in enumerate(events):
        if event["task"] != step_expect["task"]:
            continue
        if event["from_step"] != step_expect["from_step"]:
            continue
        if event["to_step"] != step_expect["to_step"]:
            continue
        if event["reason"] != step_expect["reason"]:
            continue
        return index, event
    return None, None


def format_tick_window(tick_expect: Dict[str, Any]) -> str:
    tick_min = tick_expect.get("min")
    tick_max = tick_expect.get("max")
    if tick_min is None and tick_max is None:
        return "any"
    if tick_min is not None and tick_max is not None:
        return f"[{tick_min}, {tick_max}]"
    if tick_min is not None:
        return f">= {tick_min}"
    return f"<= {tick_max}"


def evaluate_check(check: Dict[str, Any], events: List[Dict[str, Any]]) -> Dict[str, Any]:
    check_id = check.get("id", "unknown")
    axis = check.get("axis", "unknown")
    signal = check.get("signal", "unknown")
    step_expect = check.get("step", {})
    tick_expect = check.get("tick", {})

    result: Dict[str, Any] = {
        "id": check_id,
        "description": check.get("description", ""),
        "axis": axis,
        "signal": signal,
        "expected": {
            "step": {
                "task": step_expect.get("task"),
                "from_step": step_expect.get("from_step"),
                "to_step": step_expect.get("to_step"),
                "reason": step_expect.get("reason"),
            },
            "tick_window": {
                "min": tick_expect.get("min"),
                "max": tick_expect.get("max"),
            },
        },
        "observed": None,
        "passed": False,
        "message": "",
    }

    needed = ("task", "from_step", "to_step", "reason")
    missing = [k for k in needed if k not in step_expect]
    if missing:
        result["message"] = f"invalid assertion spec: missing step.{missing}"
        return result

    _, matched = find_matching_event(events, step_expect)
    if matched is None:
        result["message"] = (
            f"event not found for axis={axis} signal={signal} step={step_expect}"
        )
        return result

    observed = {
        "index": matched["index"],
        "tick": matched["tick"],
        "task": matched["task"],
        "from_step": matched["from_step"],
        "to_step": matched["to_step"],
        "reason": matched["reason"],
        "timestamp_ms": matched.get("timestamp_ms"),
    }
    result["observed"] = observed

    tick_min = tick_expect.get("min")
    tick_max = tick_expect.get("max")
    tick = matched["tick"]
    if tick_min is not None and tick < tick_min:
        result["message"] = (
            f"tick {tick} is earlier than expected window {format_tick_window(tick_expect)}"
        )
        return result
    if tick_max is not None and tick > tick_max:
        result["message"] = (
            f"tick {tick} is later than expected window {format_tick_window(tick_expect)}"
        )
        return result

    result["passed"] = True
    result["message"] = (
        f"axis={axis} signal={signal} satisfied at tick={tick} "
        f"step={matched['from_step']}->{matched['to_step']}"
    )
    return result


def build_report(
    case_id: str,
    spec_path: Path,
    trace_path: Path,
    results: List[Dict[str, Any]],
) -> Dict[str, Any]:
    passed = all(item["passed"] for item in results)
    first_failure = next((item for item in results if not item["passed"]), None)
    return {
        "case_id": case_id,
        "spec": str(spec_path),
        "trace": str(trace_path),
        "passed": passed,
        "check_count": len(results),
        "pass_count": sum(1 for item in results if item["passed"]),
        "results": results,
        "first_failure_context": first_failure,
    }


def main() -> int:
    args = parse_args()
    spec_path = Path(args.spec)
    trace_path = Path(args.trace)
    out_path = Path(args.out)

    spec = load_json(spec_path)
    case_id = spec.get("case_id", "unknown")
    checks = spec.get("checks", [])
    if not isinstance(checks, list):
        raise SystemExit(f"invalid checks list in spec: {spec_path}")

    events = load_trace(trace_path)
    results = [evaluate_check(check, events) for check in checks]
    report = build_report(case_id, spec_path, trace_path, results)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8") as f:
        json.dump(report, f, indent=2, ensure_ascii=False)
        f.write("\n")

    return 0 if report["passed"] else 2


if __name__ == "__main__":
    sys.exit(main())
