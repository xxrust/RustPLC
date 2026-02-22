#!/usr/bin/env python3
"""Topology scale performance gate for the 500-node/2000-edge baseline."""

from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Measure topology compile/parse/render baseline and guard CI thresholds."
    )
    parser.add_argument(
        "--topology",
        default="examples/topology_perf_500.topology.json",
        help="Topology JSON baseline fixture path",
    )
    parser.add_argument(
        "--scenario",
        default="examples/topology_perf_500.scenario.json",
        help="Component scenario fixture path",
    )
    parser.add_argument(
        "--thresholds",
        default="scripts/perf/topology_perf_thresholds.json",
        help="Thresholds JSON path",
    )
    parser.add_argument(
        "--samples",
        type=int,
        default=5,
        help="Number of measured samples per metric (after warmups)",
    )
    parser.add_argument(
        "--warmups",
        type=int,
        default=1,
        help="Warmup runs per metric before collecting samples",
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
        help="Skip `cargo build --release --bin rust_plc`",
    )
    return parser.parse_args()


def percentile_p95(values: list[float]) -> float:
    if not values:
        raise ValueError("values is empty")
    ordered = sorted(values)
    index = max(0, int(round((len(ordered) - 1) * 0.95)))
    return ordered[index]


def run_cmd(cmd: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        capture_output=True,
        check=False,
    )


def measure_command(
    label: str,
    cmd: list[str],
    cwd: Path,
    samples: int,
    warmups: int,
) -> dict[str, Any]:
    times: list[float] = []
    for i in range(samples + warmups):
        started = time.perf_counter()
        result = run_cmd(cmd, cwd)
        elapsed_ms = (time.perf_counter() - started) * 1000.0
        if result.returncode != 0:
            raise RuntimeError(
                f"{label} failed with exit code {result.returncode}\n"
                f"Command: {' '.join(cmd)}\n"
                f"STDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
            )
        if i >= warmups:
            times.append(elapsed_ms)
    return {
        "samples_ms": [round(v, 3) for v in times],
        "mean_ms": round(statistics.mean(times), 3),
        "p95_ms": round(percentile_p95(times), 3),
        "min_ms": round(min(times), 3),
        "max_ms": round(max(times), 3),
        "command": cmd,
    }


def normalize_endpoint(raw: str) -> str:
    idx = raw.find(".")
    return raw[:idx] if idx >= 0 else raw


def map_component_type(raw: str, device_type: str | None) -> str:
    if isinstance(device_type, str) and device_type:
        lowered = device_type.lower()
        if lowered in ("digital_input", "analog_input"):
            return "input_terminal"
        if lowered in ("digital_output", "analog_output"):
            return "output_terminal"
    lowered = raw.lower()
    if "cylinder" in lowered:
        return "cylinder"
    if "sensor" in lowered:
        return "sensor"
    if "switch" in lowered:
        return "switch"
    if "stepper" in lowered or "motor" in lowered:
        return "stepper_pd"
    return "generic"


def edge_signal_label(
    source_handle: str | None,
    target_handle: str | None,
    existing: Any,
) -> str | None:
    if isinstance(existing, str) and existing.strip():
        return existing.strip()
    if isinstance(source_handle, str) and source_handle.strip():
        return source_handle.strip()
    if isinstance(target_handle, str) and target_handle.strip():
        return target_handle.strip()
    return None


def to_canvas_topology(data: dict[str, Any]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    nodes: list[dict[str, Any]] = []
    for idx, comp in enumerate(data.get("components", [])):
        comp_id = str(comp.get("id", ""))
        params = comp.get("params") if isinstance(comp.get("params"), dict) else {}
        node_type = map_component_type(
            str(comp.get("component_id") or comp.get("type") or "generic"),
            params.get("device_type") if isinstance(params, dict) else None,
        )
        position = comp.get("position")
        if not isinstance(position, dict):
            position = {"x": 150 + (idx % 3) * 200, "y": 100 + (idx // 3) * 160}
        node_data = {
            **params,
            "label": comp_id,
            "type": node_type,
            "status": "idle",
        }
        nodes.append(
            {
                "id": comp_id,
                "type": node_type,
                "position": position,
                "data": node_data,
            }
        )

    edges: list[dict[str, Any]] = []
    for idx, conn in enumerate(data.get("connections", [])):
        source_raw = str(conn.get("from", ""))
        target_raw = str(conn.get("to", ""))
        source_handle = conn.get("from_port")
        target_handle = conn.get("to_port")
        edge: dict[str, Any] = {
            "id": f"e-{idx}",
            "source": normalize_endpoint(source_raw),
            "target": normalize_endpoint(target_raw),
        }
        relation = conn.get("relation")
        if isinstance(relation, str) and relation:
            edge["data"] = {"relation": relation}
        if isinstance(source_handle, str) and source_handle:
            edge["sourceHandle"] = source_handle
        if isinstance(target_handle, str) and target_handle:
            edge["targetHandle"] = target_handle
        label = edge_signal_label(
            source_handle if isinstance(source_handle, str) else None,
            target_handle if isinstance(target_handle, str) else None,
            conn.get("signal"),
        )
        if label:
            edge["label"] = label
        edges.append(edge)

    return nodes, edges


def measure_render_transform(
    topology: dict[str, Any],
    samples: int,
    warmups: int,
) -> dict[str, Any]:
    times: list[float] = []
    nodes_count = 0
    edges_count = 0
    for i in range(samples + warmups):
        started = time.perf_counter()
        nodes, edges = to_canvas_topology(topology)
        elapsed_ms = (time.perf_counter() - started) * 1000.0
        nodes_count = len(nodes)
        edges_count = len(edges)
        if i >= warmups:
            times.append(elapsed_ms)

    return {
        "samples_ms": [round(v, 3) for v in times],
        "mean_ms": round(statistics.mean(times), 3),
        "p95_ms": round(percentile_p95(times), 3),
        "min_ms": round(min(times), 3),
        "max_ms": round(max(times), 3),
        "nodes": nodes_count,
        "edges": edges_count,
    }


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise RuntimeError(f"Missing file: {path}") from exc
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"Invalid JSON at {path}: {exc}") from exc


def main() -> int:
    args = parse_args()
    repo_root = Path(__file__).resolve().parent.parent
    topology_path = (repo_root / args.topology).resolve()
    scenario_path = (repo_root / args.scenario).resolve()
    thresholds_path = (repo_root / args.thresholds).resolve()

    topology_json = load_json(topology_path)
    thresholds = load_json(thresholds_path)

    expected_components = int(thresholds["fixture"]["components"])
    expected_connections = int(thresholds["fixture"]["connections"])
    actual_components = len(topology_json.get("components", []))
    actual_connections = len(topology_json.get("connections", []))
    if actual_components != expected_components or actual_connections != expected_connections:
        raise RuntimeError(
            "Baseline fixture shape mismatch: "
            f"expected {expected_components} components/{expected_connections} connections, "
            f"got {actual_components} components/{actual_connections} connections"
        )

    if not args.skip_build:
        build = run_cmd(["cargo", "build", "--release", "--bin", "rust_plc"], repo_root)
        if build.returncode != 0:
            raise RuntimeError(
                "Failed to build release binary before perf gate.\n"
                f"STDOUT:\n{build.stdout}\nSTDERR:\n{build.stderr}"
            )

    rust_plc_bin = repo_root / "target" / "release" / "rust_plc"
    if not rust_plc_bin.exists():
        raise RuntimeError(f"Missing release binary: {rust_plc_bin}")

    parse_metrics = measure_command(
        "parse_validate",
        [
            str(rust_plc_bin),
            "component-topology-validate",
            str(topology_path),
            "--output",
            "json",
        ],
        repo_root,
        args.samples,
        args.warmups,
    )
    compile_metrics = measure_command(
        "compile_simulate",
        [
            str(rust_plc_bin),
            "component-sim",
            str(topology_path),
            "--scenario",
            str(scenario_path),
            "--output",
            "json",
        ],
        repo_root,
        args.samples,
        args.warmups,
    )
    render_metrics = measure_render_transform(topology_json, args.samples, args.warmups)

    metrics = {
        "parse_validate": parse_metrics,
        "compile_simulate": compile_metrics,
        "render_transform": render_metrics,
    }

    limits = thresholds["thresholds_ms_p95"]
    regressions: list[str] = []
    for key in ("parse_validate", "compile_simulate", "render_transform"):
        p95 = float(metrics[key]["p95_ms"])
        limit = float(limits[key])
        if p95 > limit:
            regressions.append(
                f"{key} p95 {p95:.3f}ms exceeds threshold {limit:.3f}ms"
            )

    payload = {
        "schema_version": 1,
        "fixture": {
            "topology": str(topology_path.relative_to(repo_root)),
            "scenario": str(scenario_path.relative_to(repo_root)),
            "components": actual_components,
            "connections": actual_connections,
        },
        "samples": args.samples,
        "warmups": args.warmups,
        "thresholds_ms_p95": limits,
        "metrics": metrics,
        "status": "pass" if not regressions else "fail",
        "regressions": regressions,
    }

    if args.output == "json":
        print(json.dumps(payload, indent=2, ensure_ascii=False))
    else:
        print(
            "topology-perf-gate: "
            f"components={actual_components} connections={actual_connections} "
            f"samples={args.samples} warmups={args.warmups}"
        )
        for key in ("parse_validate", "compile_simulate", "render_transform"):
            metric = metrics[key]
            print(
                f"- {key}: mean={metric['mean_ms']:.3f}ms "
                f"p95={metric['p95_ms']:.3f}ms "
                f"min={metric['min_ms']:.3f}ms "
                f"max={metric['max_ms']:.3f}ms "
                f"threshold={float(limits[key]):.3f}ms"
            )
        if regressions:
            print("topology-perf-gate: REGRESSION")
            for issue in regressions:
                print(f"  - {issue}")

    if regressions:
        for issue in regressions:
            # GitHub Actions warning annotation
            print(f"::warning title=Topology Performance Regression::{issue}")
        return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # pragma: no cover - script entrypoint
        print(f"topology-perf-gate: ERROR: {exc}", file=sys.stderr)
        raise SystemExit(1)
