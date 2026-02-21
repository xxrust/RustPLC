#!/usr/bin/env python3
"""Migrate legacy `connected_to` attributes in PLC files.

Default mode is dry-run and prints migration/manual-review hints.
Use --write to persist changes in place.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
from typing import List, Sequence, Tuple

KEY = "connected_to"
REPLACEMENT = "driven_by"


@dataclass
class ManualReview:
    path: Path
    line: int
    reason: str
    snippet: str


def split_code_and_comment(line: str) -> Tuple[str, str]:
    in_single = False
    in_double = False
    escape = False
    for idx, ch in enumerate(line):
        if escape:
            escape = False
            continue
        if ch == "\\":
            escape = True
            continue
        if ch == "'" and not in_double:
            in_single = not in_single
            continue
        if ch == '"' and not in_single:
            in_double = not in_double
            continue
        if ch == "#" and not in_single and not in_double:
            return line[:idx], line[idx:]
    return line, ""


def find_value_bounds(code: str, colon_idx: int) -> Tuple[int, int]:
    start = colon_idx + 1
    while start < len(code) and code[start].isspace():
        start += 1

    idx = start
    bracket_depth = 0
    in_single = False
    in_double = False
    escape = False
    while idx < len(code):
        ch = code[idx]
        if escape:
            escape = False
            idx += 1
            continue
        if ch == "\\":
            escape = True
            idx += 1
            continue
        if ch == "'" and not in_double:
            in_single = not in_single
            idx += 1
            continue
        if ch == '"' and not in_single:
            in_double = not in_double
            idx += 1
            continue
        if in_single or in_double:
            idx += 1
            continue

        if ch == "[":
            bracket_depth += 1
            idx += 1
            continue
        if ch == "]":
            bracket_depth = max(0, bracket_depth - 1)
            idx += 1
            continue

        if bracket_depth == 0 and ch in ",}":
            break

        idx += 1

    end = idx
    while end > start and code[end - 1].isspace():
        end -= 1
    return start, end


def migrate_line(code: str) -> Tuple[str, List[str]]:
    reviews: List[str] = []
    cursor = 0
    out = ""

    while cursor < len(code):
        key_idx = code.find(KEY, cursor)
        if key_idx == -1:
            out += code[cursor:]
            break

        key_start_ok = key_idx == 0 or not (
            code[key_idx - 1].isalnum() or code[key_idx - 1] == "_"
        )
        key_end = key_idx + len(KEY)
        if not key_start_ok or (
            key_end < len(code) and (code[key_end].isalnum() or code[key_end] == "_")
        ):
            out += code[cursor : key_idx + len(KEY)]
            cursor = key_idx + len(KEY)
            continue

        probe = key_end
        while probe < len(code) and code[probe].isspace():
            probe += 1
        if probe >= len(code) or code[probe] != ":":
            out += code[cursor : key_idx + len(KEY)]
            cursor = key_idx + len(KEY)
            continue

        val_start, val_end = find_value_bounds(code, probe)
        value = code[val_start:val_end].strip()
        if not value:
            reviews.append("empty value")
            out += code[cursor : key_idx + len(KEY)]
            cursor = key_idx + len(KEY)
            continue

        # Bracketed lists were legal in legacy DSL but need manual split into modern fields.
        if value.startswith("[") and value.endswith("]"):
            reviews.append("list value requires manual split")
            out += code[cursor : key_idx + len(KEY)]
            cursor = key_idx + len(KEY)
            continue

        out += code[cursor:key_idx]
        out += REPLACEMENT
        cursor = key_idx + len(KEY)

    return out, reviews


def migrate_content(path: Path, content: str) -> Tuple[str, List[ManualReview], int]:
    lines = content.splitlines(keepends=True)
    migrated_lines: List[str] = []
    manual_reviews: List[ManualReview] = []
    replacements = 0

    for line_no, line in enumerate(lines, start=1):
        code, comment = split_code_and_comment(line)
        migrated_code, reasons = migrate_line(code)
        if reasons:
            manual_reviews.append(
                ManualReview(
                    path=path,
                    line=line_no,
                    reason="; ".join(sorted(set(reasons))),
                    snippet=line.rstrip("\n"),
                )
            )
        if migrated_code != code:
            replacements += 1
        migrated_lines.append(migrated_code + comment)

    return "".join(migrated_lines), manual_reviews, replacements


def collect_targets(raw_paths: Sequence[str]) -> List[Path]:
    targets: List[Path] = []
    for raw in raw_paths:
        path = Path(raw)
        if path.is_file():
            targets.append(path)
            continue
        if path.is_dir():
            targets.extend(sorted(p for p in path.rglob("*.plc") if p.is_file()))
            continue
        raise FileNotFoundError(f"path not found: {raw}")

    # Stable de-dup by resolved path while preserving first-seen order.
    seen = set()
    unique_targets: List[Path] = []
    for path in targets:
        key = path.resolve()
        if key in seen:
            continue
        seen.add(key)
        unique_targets.append(path)
    return unique_targets


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Migrate legacy `connected_to` attributes to `driven_by` in PLC files."
    )
    parser.add_argument(
        "paths",
        nargs="+",
        help="PLC file(s) or directories (directories are scanned for *.plc)",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="Write migrated content back to disk (default: dry-run)",
    )
    parser.add_argument(
        "--strict-manual",
        action="store_true",
        help="Return non-zero when manual review items are detected",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        targets = collect_targets(args.paths)
    except FileNotFoundError as exc:
        print(f"[migrate-connected-to] ERROR: {exc}")
        return 2

    if not targets:
        print("[migrate-connected-to] No PLC files found.")
        return 0

    total_replacements = 0
    manual_reviews: List[ManualReview] = []

    for path in targets:
        original = path.read_text(encoding="utf-8")
        migrated, reviews, replacements = migrate_content(path, original)
        manual_reviews.extend(reviews)
        total_replacements += replacements

        if replacements == 0:
            continue

        if args.write:
            path.write_text(migrated, encoding="utf-8")
            print(f"[migrate-connected-to] updated {path} ({replacements} replacement lines)")
        else:
            print(f"[migrate-connected-to] would update {path} ({replacements} replacement lines)")

    if total_replacements == 0:
        print("[migrate-connected-to] No legacy attributes found.")

    if manual_reviews:
        print("[migrate-connected-to] Manual review required for unresolved legacy lines:")
        for item in manual_reviews:
            print(f"  - {item.path}:{item.line} ({item.reason})")
            print(f"    {item.snippet}")

    if args.strict_manual and manual_reviews:
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
