# Topology Signal Direction Refactor

This page documents the topology semantics refactor now reflected in the current repository (original rollout: US-001 ~ US-016).

---

## Background

The original DSL used `connected_to` to express device relationships, but this field was ambiguous — it conflated drive relationships, signal reporting, and detection into a single keyword. As topology complexity grew (MIMO, multi-sensor, cross-zone), the ambiguity caused inconsistencies in IR construction, causality verification, and frontend rendering.

---

## What Changed

### DSL: `connected_to` → explicit relation fields

| Old | New | Meaning |
|-----|-----|---------|
| `connected_to: valve_A` | `driven_by: valve_A` | Actuator is driven by upstream device |
| `connected_to: X0` | `reports_to: X0` | Sensor reports signal to I/O point |
| `connected_to: cyl_A` | `detects: cyl_A` | Sensor detects state of target device |

All topology edges now flow **producer → consumer** (signal source to signal sink).

### Ports as first-class citizens

Devices now declare typed ports:

```json
{
  "id": "extend_out",
  "type": "pneumatic",
  "role": "producer"
}
```

Port roles: `producer` | `consumer` | `bidirectional`
Port types: `digital` | `analog` | `pneumatic` | `logical` | `generic`

Connections reference `from_port` / `to_port` explicitly, enabling MIMO topologies (one-to-many, many-to-one, many-to-many).

### Multi-dimensional tags

Devices support structured tags for grouping, risk classification, and location:

```plc
device cyl_press: cylinder {
    driven_by: valve_press,
    tags: {
        functional_group: "press_unit",
        danger_level: "high",
        location_group: "line_a/cell_2/station_7"
    }
}
```

Tag dimensions:
- `functional_group` — logical function grouping
- `danger_level` — risk classification (`low` / `medium` / `high`)
- `location_group` — hierarchical physical location (`line/cell/station`)

---

## Migration

### Automated migration tool

```bash
python3 scripts/migrate_connected_to.py --input examples/ --output examples/
```

Items that cannot be auto-migrated are flagged with a human-confirmation prompt.

### CI regression guard

CI now rejects any new `connected_to` usage:

```bash
bash scripts/ci_no_connected_to_regression.sh
```

---

## Tag Rule Engine

Tag rules are declared in the topology JSON under `tag_rules`:

```json
{
  "tag_rules": {
    "danger_level": {
      "dual_channel_levels": ["high"]
    },
    "functional_group": {
      "mode": "within_only"
    },
    "location_group": {
      "mode": "allow_any",
      "allowed_cross_zone_pairs": [["line_a/cell_1", "line_a/cell_2"]]
    }
  }
}
```

Rule violations produce structured errors with `code/path/message`.

---

## API Changes

`parse-plc` and topology API responses now include:

```json
{
  "relations": [
    {
      "from": "valve_A",
      "to": "cyl_A",
      "relation": "driven_by",
      "from_port": "output",
      "to_port": "drive_in",
      "signal": "pneumatic"
    }
  ],
  "nodes": [
    {
      "id": "cyl_A",
      "ports": [...],
      "tags": { "functional_group": "press_unit", "danger_level": "high" }
    }
  ]
}
```

---

## Frontend

- Connections are bound to `sourceHandle` / `targetHandle` matching port IDs
- Port contract covers: `cylinder` / `sensor` / `switch` / `stepper` / `generic`
- Missing port metadata shows degraded style + warning
- Tag panel supports filter, group highlight, and `location_group` one-click navigation

---

## Performance Gate

A 500-node / 2000-edge baseline fixture guards scale regressions:

```bash
python3 scripts/topology_perf_gate.py --output human
```

Thresholds (`scripts/perf/topology_perf_thresholds.json`):

| Path | p95 limit |
|------|-----------|
| `parse_validate` | 250 ms |
| `compile_simulate` | 400 ms |
| `render_transform` | 80 ms |

---

## Semantic Diff

The `component-topology-diff` module computes node/port/relation/tag-level diffs between two topology snapshots and outputs an impact analysis (affected rules, tests, modules). Output is suitable for audit records.

---

## Related Files

- `src/ast/mod.rs` — `TopologyConnection`, `DevicePort`, `DeviceTags`, `TopologyRelation`
- `src/component_topology.rs` — `ComponentTagRules`, tag rule validation
- `src/semantic/mod.rs` — producer → consumer graph construction
- `src/verification/causality.rs` — updated BFS traversal
- `scripts/migrate_connected_to.py` — migration tool
- `scripts/ci_no_connected_to_regression.sh` — CI guard
- `scripts/topology_perf_gate.py` — performance gate
- `docs/已实现/topology_perf_baseline.md` — baseline fixture docs
- `docs/已实现/testing_inventory_matrix.md` — test coverage matrix
- `tests/component_topology_diff.rs` — semantic diff tests
- `tests/component_topology_validate.rs` — tag rule contract tests
