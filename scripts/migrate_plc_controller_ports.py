#!/usr/bin/env python3
"""
Heuristic migration helper:
- convert legacy topology declarations like `device X0: digital_input`
  into one `device plc_main: plc { model_ref: openplc_softplc }`
- rewrite `relation { from: X0, ... }` / `to: Y0` into `plc_main.X0`/`plc_main.Y0`

This script intentionally keeps task/action statements unchanged. Those keep
working because the compiler preprocess expands `plc_main.<port>` back into
internal IO endpoint nodes.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path
from typing import List, Tuple


TOPOLOGY_HEADER_RE = re.compile(r"^\s*\[\s*topology\s*\]\s*$", re.IGNORECASE)
SECTION_HEADER_RE = re.compile(r"^\s*\[[^\]]+\]\s*$")
LEGACY_IO_DEVICE_RE = re.compile(
    r"^\s*device\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(digital_input|digital_output|analog_input|analog_output)\b.*$",
    re.IGNORECASE,
)
PHYSICAL_IO_NAME_RE = re.compile(r"^(X\d+|Y\d+|AI\d+|AO\d+)$")
RELATION_ENDPOINT_RE = re.compile(r"\b(from|to)\s*:\s*(X\d+|Y\d+|AI\d+|AO\d+)\b")

PROFILE_DECLARATION = (
    'device plc_main: plc { purpose: "控制器本体与端口映射", model_ref: openplc_softplc }'
)


def rewrite_relations(text: str) -> str:
    def repl(match: re.Match[str]) -> str:
        key = match.group(1)
        endpoint = match.group(2)
        return f"{key}: plc_main.{endpoint}"

    return RELATION_ENDPOINT_RE.sub(repl, text)


def migrate(content: str) -> Tuple[str, List[str]]:
    lines = content.splitlines()
    out: List[str] = []
    migrated_ports: List[str] = []

    in_topology = False
    inserted_plc = False

    for line in lines:
        if TOPOLOGY_HEADER_RE.match(line):
            in_topology = True
            inserted_plc = False
            out.append(line)
            continue

        if in_topology and SECTION_HEADER_RE.match(line):
            if migrated_ports and not inserted_plc:
                out.append(PROFILE_DECLARATION)
                inserted_plc = True
            in_topology = False
            out.append(line)
            continue

        if in_topology:
            match = LEGACY_IO_DEVICE_RE.match(line)
            if match:
                name = match.group(1)
                if PHYSICAL_IO_NAME_RE.match(name):
                    migrated_ports.append(name)
                    # Keep declarations that carry attributes (e.g. range/external/subtype)
                    # and only rely on controller ports to replace plain X/Y/AI/AO declarations.
                    if "{" in line:
                        out.append(line)
                    continue
            out.append(line)
            continue

        out.append(line)

    if in_topology and migrated_ports and not inserted_plc:
        out.append(PROFILE_DECLARATION)

    seen = set()
    ordered_ports: List[str] = []
    for name in migrated_ports:
        if name in seen:
            continue
        seen.add(name)
        ordered_ports.append(name)

    rewritten = "\n".join(out)
    if content.endswith("\n"):
        rewritten += "\n"
    rewritten = rewrite_relations(rewritten)
    return rewritten, ordered_ports


def main() -> int:
    parser = argparse.ArgumentParser(description="Migrate legacy X/Y/AI/AO device declarations.")
    parser.add_argument("input", type=Path, help="input .plc file")
    parser.add_argument("-o", "--output", type=Path, help="output .plc file")
    parser.add_argument("--in-place", action="store_true", help="overwrite input file")
    args = parser.parse_args()

    if args.output and args.in_place:
        parser.error("--output and --in-place are mutually exclusive")
    if not args.output and not args.in_place:
        parser.error("choose --output <file> or --in-place")

    src = args.input.read_text(encoding="utf-8")
    rewritten, ports = migrate(src)

    if not ports:
        print("No legacy X/Y/AI/AO device declarations found; no changes made.")
        return 0

    target = args.input if args.in_place else args.output
    assert target is not None
    target.write_text(rewritten, encoding="utf-8")
    print(f"Migrated {len(ports)} ports: {', '.join(ports)}")
    print(f"Wrote: {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
