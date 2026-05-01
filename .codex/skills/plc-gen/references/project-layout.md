# plc-gen Project Layout

## Role

This reference explains how `plc-gen` should choose a project layout for real delivery work.

The key rule is:

- do not choose a layout only by file count
- choose it by delivery layer and ownership boundary

## Delivery Layers Come First

RustPLC projects may be delivered as:

- `module`
- `station`
- `line`

These are not just documentation labels.
They are the primary authoring boundary.

Selection rule:

- if the user is delivering one reusable mechanism or unit, start from `module`
- if the user is delivering one independently testable process cell, start from `station`
- if the user is delivering multi-station integration, start from `line`

For the frozen architecture source behind this rule, read:

- `docs/architecture/delivery_layer_framework.md`

## Current Best Practical Shape

Today, the best practical shape is a two-layer structure:

1. `authoring tree`
2. `compile surface`

The authoring tree follows delivery assets:

- `module`
- `station`
- `line`

The compile surface remains the structured fragment layout used by the current scaffold:

- `rustplc.bundle.toml`
- `00_topology/`
- `01_init/`
- `02_process/`
- `03_constraints/`
- `04_faults/`
- `05_supervision/`
- `06_manual/`
- `07_hmi/`

Do not confuse the compile surface with the whole project architecture.

## Mandatory Document Set Per Delivery Asset

Each `module`, `station`, or `line` asset must carry:

- `*.system.md`
- `*.architecture.md`
- `*.intent_alignment.contract.json`
- `*.verification.md`

If these are missing, the delivery asset is structurally incomplete even if the PLC compiles.

## Recommended Authored Overlay Shape

```text
rustplc.project.toml
plc/main.system.md
rustplc.bundle.toml
00_topology/
01_init/
02_process/
03_constraints/
04_faults/
05_supervision/
06_manual/
07_hmi/
scenarios/nominal/normal.yaml
docs/
  system.md or <asset>.system.md
  architecture.md or <asset>.architecture.md
  verification.md or <asset>.verification.md
rustplc.bundle.intent_alignment.contract.json
```

## Structured Fragment Layout Still Matters

For the compileable source set, keep the scaffolded phase layout unless the existing project has already chosen another explicit bundle boundary.

Why this still matters:

- it splits by semantic ownership rather than writing order
- it remains easy to compile, review, and test
- it prevents early collapse into one spaghetti PLC

## Naming Rule For Flattened Fragments

When flattening delivery assets into one compile surface, preserve ownership in file names.

Examples:

- `00_topology/module_pick_head_devices.plc`
- `00_topology/station_load_devices.plc`
- `00_topology/line_layout.plc`
- `02_process/station_load_cycle.plc`
- `04_faults/module_pick_head_faults.plc`

This keeps lower-layer ownership visible even when the compiler sees a flat source entry.

## Scaffold Guidance

For new complex projects, `rust_plc new <project_dir> --layout structured-fragments` is still the right compile-surface starting point.

But the skill should not stop there.

After scaffold:

1. classify the delivery asset as `module`, `station`, or `line`
2. create the layer-specific document set
3. decide whether the scaffolded `rustplc.bundle.toml` remains the authoritative source entry
4. only then fill the structured phase files

## Independent Validation Requirement

Each delivery asset should be independently runnable and checkable.

That means each asset should own:

- its own source entry
- its own scenario set
- its own intent contract
- its own validation command path

Do not rely on a line-level run to prove a module or station is correct.

## Special Rule For Workpiece Flow

If the delivered asset moves a real part, first-class workpiece semantics are mandatory at that layer.

Examples:

- module: only if the module itself owns a real acquire/transfer/finish boundary
- station: usually yes for station ingress/process/egress
- line: yes for inter-station handoff and route closure

Do not leave workpiece semantics only at line level when station-level logic truly consumes and hands off parts.

## What To Show The User First

For a delivery-asset-oriented project, show the user in this order:

1. the layer-specific `*.system.md`
2. the layer-specific `*.architecture.md`
3. the layer-specific source entry
4. the layer-specific nominal scenario
5. the layer-specific intent contract

Artifact directories such as `out/` remain generated outputs, not the first files to hand-edit.
