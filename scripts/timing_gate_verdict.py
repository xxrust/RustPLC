#!/usr/bin/env python3
"""Evaluate RP2040 timing evidence against optional thresholds."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate timing gate verdict artifact from timing_report.json"
    )
    parser.add_argument("--timing-report", required=True, help="Path to timing_report.json")
    parser.add_argument("--out", required=True, help="Output verdict JSON path")
    parser.add_argument("--max-p99-exec-us", type=int, default=None)
    parser.add_argument("--max-overrun-count", type=int, default=None)
    return parser.parse_args()


def build_base_payload(
    timing_report: Path, max_p99_exec_us: int | None, max_overrun_count: int | None
) -> dict[str, Any]:
    return {
        "timing_report": str(timing_report),
        "thresholds": {
            "max_p99_exec_us": max_p99_exec_us,
            "max_overrun_count": max_overrun_count,
        },
        "observed": {
            "count": 0,
            "exec_us_p99": None,
            "overrun_count": None,
        },
        "violations": [],
        "status": "unknown",
        "status_code": 0,
        "message": "",
    }


def main() -> int:
    args = parse_args()
    timing_report = Path(args.timing_report)
    out = Path(args.out)
    thresholds_enabled = (
        args.max_p99_exec_us is not None or args.max_overrun_count is not None
    )

    payload = build_base_payload(
        timing_report=timing_report,
        max_p99_exec_us=args.max_p99_exec_us,
        max_overrun_count=args.max_overrun_count,
    )

    if not timing_report.exists():
        if thresholds_enabled:
            payload["status"] = "fail"
            payload["status_code"] = 2
            payload["message"] = "timing_report.json is missing while thresholds are configured"
        else:
            payload["status"] = "no_data"
            payload["status_code"] = 0
            payload["message"] = "timing_report.json is missing and no thresholds are configured"
        out.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        return int(payload["status_code"])

    report = json.loads(timing_report.read_text(encoding="utf-8"))
    count = int(report.get("count", 0) or 0)
    p99 = int(report.get("exec_us_p99", 0) or 0)
    overrun_count = int(report.get("overrun_count", 0) or 0)

    payload["observed"] = {
        "count": count,
        "exec_us_p99": p99,
        "overrun_count": overrun_count,
    }

    violations: list[dict[str, int]] = []
    if args.max_p99_exec_us is not None and p99 > args.max_p99_exec_us:
        violations.append(
            {
                "metric": "exec_us_p99",
                "observed": p99,
                "limit": args.max_p99_exec_us,
            }
        )
    if args.max_overrun_count is not None and overrun_count > args.max_overrun_count:
        violations.append(
            {
                "metric": "overrun_count",
                "observed": overrun_count,
                "limit": args.max_overrun_count,
            }
        )

    payload["violations"] = violations

    if thresholds_enabled:
        if count <= 0:
            payload["status"] = "fail"
            payload["status_code"] = 2
            payload["message"] = "timing_report.json has no timing samples"
        elif violations:
            payload["status"] = "fail"
            payload["status_code"] = 2
            payload["message"] = "realtime threshold exceeded"
        else:
            payload["status"] = "pass"
            payload["status_code"] = 0
            payload["message"] = "realtime thresholds satisfied"
    else:
        if count <= 0:
            payload["status"] = "no_data"
            payload["status_code"] = 0
            payload["message"] = "timing samples unavailable (thresholds not configured)"
        else:
            payload["status"] = "not_configured"
            payload["status_code"] = 0
            payload["message"] = "thresholds are not configured"

    out.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return int(payload["status_code"])


if __name__ == "__main__":
    raise SystemExit(main())
