#!/usr/bin/env python3
"""Validate abnormal-exit matrix evidence for RP2040 safety classes."""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any, Dict, List


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate abnormal-exit matrix evidence (A/B/C automated, D manual)."
    )
    parser.add_argument("--matrix", required=True, help="Path to abnormal-exit matrix JSON")
    parser.add_argument(
        "--evidence-dir",
        required=True,
        help="Directory containing per-class evidence JSON files (A.json/B.json/...)",
    )
    parser.add_argument("--out", required=True, help="Output report JSON path")
    parser.add_argument(
        "--require-classes",
        default="A,B,C",
        help="Comma-separated classes that must pass automated verification",
    )
    return parser.parse_args()


def load_json(path: Path) -> Dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8") as f:
            return json.load(f)
    except FileNotFoundError as err:
        raise SystemExit(f"file not found: {path}") from err
    except json.JSONDecodeError as err:
        raise SystemExit(f"invalid JSON {path}: {err}") from err


def values_match(expected: Any, observed: Any) -> bool:
    if isinstance(expected, float) or isinstance(observed, float):
        try:
            return math.isclose(float(expected), float(observed), rel_tol=1e-6, abs_tol=1e-6)
        except (TypeError, ValueError):
            return False
    return expected == observed


def _is_nonempty_string(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())


def validate_evidence(
    class_spec: Dict[str, Any], evidence: Dict[str, Any], *, expected_verdict: str = "pass"
) -> List[str]:
    errors: List[str] = []
    class_id = class_spec.get("id")

    if evidence.get("class") != class_id:
        errors.append(
            f"evidence.class mismatch: expected {class_id}, got {evidence.get('class')}"
        )

    trigger = evidence.get("trigger")
    if not isinstance(trigger, dict):
        errors.append("missing trigger object")
    else:
        expected_method = class_spec.get("trigger_method")
        if trigger.get("method") != expected_method:
            errors.append(
                "trigger.method mismatch: "
                f"expected {expected_method}, got {trigger.get('method')}"
            )

    observed_outputs = evidence.get("observed_outputs")
    if not isinstance(observed_outputs, list):
        errors.append("missing observed_outputs list")
        observed_outputs = []

    outputs_by_channel: Dict[str, Dict[str, Any]] = {}
    for row in observed_outputs:
        if isinstance(row, dict) and "channel" in row:
            outputs_by_channel[str(row["channel"])] = row

    for expected_output in class_spec.get("expected_outputs", []):
        channel = str(expected_output.get("channel"))
        observed = outputs_by_channel.get(channel)
        if observed is None:
            errors.append(f"missing observed output for channel {channel}")
            continue
        if "value" in expected_output and not values_match(
            expected_output["value"], observed.get("value")
        ):
            errors.append(
                f"output {channel} value mismatch: "
                f"expected {expected_output['value']}, got {observed.get('value')}"
            )
        if "order" in expected_output and expected_output["order"] != observed.get("order"):
            errors.append(
                f"output {channel} order mismatch: "
                f"expected {expected_output['order']}, got {observed.get('order')}"
            )

    checks = evidence.get("checks")
    if not isinstance(checks, dict):
        errors.append("missing checks object")
        checks = {}

    for check in class_spec.get("acceptance_checks", []):
        if checks.get(check) is not True:
            errors.append(f"acceptance check `{check}` is not true")

    if evidence.get("verdict") != expected_verdict:
        errors.append(
            f"verdict must be `{expected_verdict}`, got {evidence.get('verdict')}"
        )

    artifacts = evidence.get("artifacts")
    if not isinstance(artifacts, dict):
        errors.append("missing artifacts object")
    else:
        for required in ("trigger_log", "output_log"):
            if not artifacts.get(required):
                errors.append(f"artifacts.{required} is required")

    return errors


def validate_class_d_manual_artifacts(evidence: Dict[str, Any]) -> List[str]:
    errors: List[str] = []
    manual = evidence.get("class_d_manual")
    if not isinstance(manual, dict):
        return ["missing class_d_manual object"]

    for field in ("trigger", "wiring_state", "measured_result", "operator"):
        if not _is_nonempty_string(manual.get(field)):
            errors.append(f"class_d_manual.{field} must be a non-empty string")

    if manual.get("verdict") != "pass":
        errors.append(
            f"class_d_manual.verdict must be `pass`, got {manual.get('verdict')}"
        )

    attachments = manual.get("attachments")
    if not isinstance(attachments, list) or not attachments:
        errors.append("class_d_manual.attachments must be a non-empty array")
        return errors

    for idx, attachment in enumerate(attachments):
        if not isinstance(attachment, dict):
            errors.append(f"class_d_manual.attachments[{idx}] must be an object")
            continue
        for field in ("name", "path"):
            if not _is_nonempty_string(attachment.get(field)):
                errors.append(
                    f"class_d_manual.attachments[{idx}].{field} must be a non-empty string"
                )

        provenance = attachment.get("provenance")
        if not isinstance(provenance, dict):
            errors.append(
                f"class_d_manual.attachments[{idx}].provenance must be an object"
            )
            continue

        if not _is_nonempty_string(provenance.get("source_path")):
            errors.append(
                f"class_d_manual.attachments[{idx}].provenance.source_path must be a non-empty string"
            )

        for optional_field in ("digest_sha256", "captured_by", "captured_at"):
            optional_value = provenance.get(optional_field)
            if optional_value is not None and not _is_nonempty_string(optional_value):
                errors.append(
                    f"class_d_manual.attachments[{idx}].provenance.{optional_field} must be a non-empty string when present"
                )

    return errors


def main() -> int:
    args = parse_args()

    matrix_path = Path(args.matrix)
    evidence_dir = Path(args.evidence_dir)
    out_path = Path(args.out)

    matrix = load_json(matrix_path)
    classes = matrix.get("classes")
    if not isinstance(classes, list):
        raise SystemExit("matrix JSON must include a `classes` array")

    class_specs: Dict[str, Dict[str, Any]] = {}
    for item in classes:
        if not isinstance(item, dict) or "id" not in item:
            raise SystemExit("matrix classes must be objects with id")
        class_specs[str(item["id"])] = item

    required_classes = [c.strip() for c in args.require_classes.split(",") if c.strip()]
    if not required_classes:
        raise SystemExit("--require-classes must include at least one class")

    overall_pass = True
    results: List[Dict[str, Any]] = []

    for class_id, class_spec in class_specs.items():
        automation = class_spec.get("automation", "auto")
        result: Dict[str, Any] = {
            "class": class_id,
            "title": class_spec.get("title", ""),
            "automation": automation,
            "required": class_id in required_classes,
        }

        if automation == "hardware_only":
            result["manual_reason"] = (
                "class is marked hardware_only; verify via independent hardware safety chain"
            )
            evidence_path = evidence_dir / f"{class_id}.json"
            if not evidence_path.exists():
                result["status"] = "missing_evidence"
                result["errors"] = [f"evidence file not found: {evidence_path}"]
                overall_pass = False
                results.append(result)
                continue

            evidence = load_json(evidence_path)
            errors = validate_evidence(class_spec, evidence, expected_verdict="manual")
            errors.extend(validate_class_d_manual_artifacts(evidence))

            if class_id in required_classes:
                errors.append("class is marked hardware_only and cannot be auto-verified")

            result["evidence"] = str(evidence_path)
            result["errors"] = errors
            result["status"] = (
                "manual_hardware_chain_validated" if not errors else "manual_hardware_chain_invalid"
            )
            if errors:
                overall_pass = False
            results.append(result)
            continue

        if class_id not in required_classes:
            result["status"] = "not_required"
            result["errors"] = []
            results.append(result)
            continue

        evidence_path = evidence_dir / f"{class_id}.json"
        if not evidence_path.exists():
            result["status"] = "missing_evidence"
            result["errors"] = [f"evidence file not found: {evidence_path}"]
            overall_pass = False
            results.append(result)
            continue

        evidence = load_json(evidence_path)
        errors = validate_evidence(class_spec, evidence)
        result["evidence"] = str(evidence_path)
        result["errors"] = errors
        result["status"] = "pass" if not errors else "fail"
        if errors:
            overall_pass = False
        results.append(result)

    # Ensure requested classes are declared in matrix.
    unknown_required = [c for c in required_classes if c not in class_specs]
    if unknown_required:
        overall_pass = False
        for class_id in unknown_required:
            results.append(
                {
                    "class": class_id,
                    "title": "",
                    "automation": "unknown",
                    "required": True,
                    "status": "unknown_class",
                    "errors": ["class not declared in matrix"],
                }
            )

    report = {
        "matrix": str(matrix_path),
        "evidence_dir": str(evidence_dir),
        "required_classes": required_classes,
        "status": "pass" if overall_pass else "fail",
        "results": results,
    }

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    return 0 if overall_pass else 2


if __name__ == "__main__":
    sys.exit(main())
