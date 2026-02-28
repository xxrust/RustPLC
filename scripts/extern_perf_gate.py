#!/usr/bin/env python3
"""Extern call performance regression gate."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Measure extern call overhead and fail when thresholds/regression limits are exceeded."
    )
    parser.add_argument(
        "--thresholds",
        default="scripts/perf/extern_perf_thresholds.json",
        help="Threshold configuration JSON path",
    )
    parser.add_argument(
        "--baseline",
        default="scripts/perf/extern_perf_baseline.json",
        help="Baseline benchmark JSON path",
    )
    parser.add_argument(
        "--benchmark-json",
        help="Use an existing benchmark payload JSON instead of running the benchmark binary",
    )
    parser.add_argument(
        "--samples",
        type=int,
        default=7,
        help="Measured samples per benchmark metric",
    )
    parser.add_argument(
        "--warmups",
        type=int,
        default=2,
        help="Warmup runs per benchmark metric",
    )
    parser.add_argument(
        "--simple-iterations",
        type=int,
        default=100000,
        help="Calls per sample for simple add benchmark",
    )
    parser.add_argument(
        "--complex-iterations",
        type=int,
        default=20000,
        help="Calls per sample for quadratic_fit benchmark",
    )
    parser.add_argument(
        "--output",
        choices=("human", "json"),
        default="human",
        help="Output format",
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="Skip `cargo build --release --bin extern_perf_bench`",
    )
    return parser.parse_args()


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise RuntimeError(f"Missing file: {path}") from exc
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"Invalid JSON at {path}: {exc}") from exc


def display_path(path: Path, repo_root: Path) -> str:
    try:
        return str(path.relative_to(repo_root))
    except ValueError:
        return str(path)


def run_cmd(cmd: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        capture_output=True,
        check=False,
    )


def load_or_run_benchmark(args: argparse.Namespace, repo_root: Path) -> dict[str, Any]:
    if args.benchmark_json:
        benchmark_path = (repo_root / args.benchmark_json).resolve()
        payload = load_json(benchmark_path)
    else:
        if not args.skip_build:
            build = run_cmd(["cargo", "build", "--release", "--bin", "extern_perf_bench"], repo_root)
            if build.returncode != 0:
                raise RuntimeError(
                    "Failed to build benchmark binary.\n"
                    f"STDOUT:\n{build.stdout}\nSTDERR:\n{build.stderr}"
                )

        bench_bin = repo_root / "target" / "release" / "extern_perf_bench"
        if not bench_bin.exists():
            raise RuntimeError(f"Missing release benchmark binary: {bench_bin}")

        result = run_cmd(
            [
                str(bench_bin),
                "--output",
                "json",
                "--samples",
                str(args.samples),
                "--warmups",
                str(args.warmups),
                "--simple-iterations",
                str(args.simple_iterations),
                "--complex-iterations",
                str(args.complex_iterations),
            ],
            repo_root,
        )
        if result.returncode != 0:
            raise RuntimeError(
                "Benchmark run failed.\n"
                f"STDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
            )
        try:
            payload = json.loads(result.stdout)
        except json.JSONDecodeError as exc:
            raise RuntimeError(
                "Benchmark output is not valid JSON.\n"
                f"STDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
            ) from exc

    if payload.get("schema_version") != 1:
        raise RuntimeError("Benchmark payload must include schema_version=1")

    metrics = payload.get("metrics_us_per_call")
    if not isinstance(metrics, dict):
        raise RuntimeError("Benchmark payload missing metrics_us_per_call")

    for metric_key in ("simple_add", "complex_quadratic_fit"):
        metric = metrics.get(metric_key)
        if not isinstance(metric, dict) or "p95_us" not in metric:
            raise RuntimeError(f"Benchmark payload missing {metric_key}.p95_us")

    return payload


def main() -> int:
    args = parse_args()
    repo_root = Path(__file__).resolve().parent.parent
    thresholds_path = (repo_root / args.thresholds).resolve()
    baseline_path = (repo_root / args.baseline).resolve()

    thresholds = load_json(thresholds_path)
    baseline = load_json(baseline_path)
    benchmark = load_or_run_benchmark(args, repo_root)

    thresholds_p95 = thresholds.get("thresholds_p95_us")
    if not isinstance(thresholds_p95, dict):
        raise RuntimeError("Threshold config missing thresholds_p95_us map")

    baseline_p95 = baseline.get("metrics_p95_us")
    if not isinstance(baseline_p95, dict):
        raise RuntimeError("Baseline config missing metrics_p95_us map")

    max_regression_pct = float(thresholds.get("max_regression_pct_vs_baseline", 0.0))

    measured = benchmark["metrics_us_per_call"]
    regressions: list[str] = []
    checks: dict[str, Any] = {}

    for metric_key in ("simple_add", "complex_quadratic_fit"):
        measured_p95 = float(measured[metric_key]["p95_us"])
        threshold_limit = float(thresholds_p95[metric_key])
        baseline_value = float(baseline_p95[metric_key])
        baseline_limit = baseline_value * (1.0 + max_regression_pct / 100.0)

        check = {
            "measured_p95_us": round(measured_p95, 3),
            "threshold_limit_us": round(threshold_limit, 3),
            "baseline_p95_us": round(baseline_value, 3),
            "baseline_regression_limit_us": round(baseline_limit, 3),
        }
        checks[metric_key] = check

        if measured_p95 > threshold_limit:
            regressions.append(
                f"{metric_key} p95 {measured_p95:.3f}us exceeds threshold {threshold_limit:.3f}us"
            )
        if measured_p95 > baseline_limit:
            regressions.append(
                f"{metric_key} p95 {measured_p95:.3f}us exceeds baseline regression limit {baseline_limit:.3f}us"
            )

    payload = {
        "schema_version": 1,
        "status": "pass" if not regressions else "fail",
        "thresholds": {
            "path": display_path(thresholds_path, repo_root),
            "max_regression_pct_vs_baseline": max_regression_pct,
        },
        "baseline": {
            "path": display_path(baseline_path, repo_root),
        },
        "checks": checks,
        "benchmark": benchmark,
        "regressions": regressions,
    }

    if args.output == "json":
        print(json.dumps(payload, indent=2, ensure_ascii=False))
    else:
        print(
            "extern-perf-gate: "
            f"samples={benchmark.get('samples')} warmups={benchmark.get('warmups')} "
            f"simple_iters={benchmark.get('simple_iterations')} "
            f"complex_iters={benchmark.get('complex_iterations')}"
        )
        for metric_key in ("simple_add", "complex_quadratic_fit"):
            check = checks[metric_key]
            print(
                f"- {metric_key}: measured_p95={check['measured_p95_us']:.3f}us "
                f"threshold={check['threshold_limit_us']:.3f}us "
                f"baseline={check['baseline_p95_us']:.3f}us "
                f"baseline_limit={check['baseline_regression_limit_us']:.3f}us"
            )
        if regressions:
            print("extern-perf-gate: REGRESSION")
            for issue in regressions:
                print(f"  - {issue}")

    if regressions:
        for issue in regressions:
            print(f"::warning title=Extern Performance Regression::{issue}", file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # pragma: no cover - script entrypoint
        print(f"extern-perf-gate: ERROR: {exc}", file=sys.stderr)
        raise SystemExit(1)
