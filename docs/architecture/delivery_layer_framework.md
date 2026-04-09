# Delivery Layer Framework

## 1. Role

This document freezes the delivery-layer framework for RustPLC projects that may be delivered as:

- a reusable `module`
- an independently testable `station`
- an integrated `line`

It exists to prevent large automation projects from collapsing into one flat PLC source set with no stable ownership boundaries.

This document is an architecture source, not a temporary migration note.

## 2. First Principles

For automation projects, "what gets delivered" is not always a whole line.

Many suppliers only deliver:

- one repeated module
- one complete station
- one line-level integration package

Therefore RustPLC should treat `module`, `station`, and `line` as first-class delivery assets rather than assuming every project starts from one monolithic top-level PLC.

## 3. Frozen Delivery Layers

### 3.1 Module

A `module` is the smallest reusable control asset.

Examples:

- pick head
- clamp pair
- align axis unit
- camera inspection unit

Module responsibilities:

- define local device semantics
- define local action/result contracts
- define local resource claims
- define local fault exits
- support independent compile, simulation, and verification

Non-responsibilities:

- whole-station scheduling
- cross-station handoff
- line-level takt balancing

### 3.2 Station

A `station` is the smallest reusable process asset.

Examples:

- loading station
- inspection station
- unloading station

Station responsibilities:

- compose one or more modules
- define station-local workpiece flow
- define station-level supervisor and mode transitions
- define station fault/recovery boundaries
- support independent compile, simulation, and verification

Non-responsibilities:

- line-wide routing
- line-wide inter-station sequencing beyond explicit handoff contracts

### 3.3 Line

A `line` is the integration asset that composes stations into a production system.

Line responsibilities:

- define top layout and inter-station handoff
- define line-level workpiece routing
- define line-level global interlocks and modes
- define line-level takt, buffering, and escalation policy
- support integrated compile, simulation, and verification

Non-responsibilities:

- restating internal station logic
- restating internal module logic

## 4. Mandatory Architecture Documents

Each delivery layer must carry its own architecture-grade document set.

The minimum required set is:

1. `*.system.md`
2. `*.architecture.md`
3. `*.intent_alignment.contract.json`
4. `*.verification.md`

These are not optional sidecars for complex delivery.

If a layer lacks its own architecture document, that layer is not considered a closed delivery asset.

## 5. Required Questions Per Layer

### 5.1 Module Architecture

`module.architecture.md` must answer:

- what semantic inputs and outputs the module exposes
- which actions are high-level semantic actions
- which results and abnormal results are explicit
- which resources are claimed locally
- which parameters are instance-time configurable
- what assumptions are required for independent testing

### 5.2 Station Architecture

`station.architecture.md` must answer:

- which modules are composed inside the station
- how workpieces flow inside the station
- which station tasks are concurrent and which are supervisory
- where station fault domains start and end
- what manual and maintenance boundaries exist
- what contract the station exposes upward to a line

### 5.3 Line Architecture

`line.architecture.md` must answer:

- station topology and inter-station handoff
- line-level workpiece routing and buffering
- global line modes and escalation
- cross-station resources or interlocks
- line-level independent verification scope
- which lower-layer contracts are being consumed

## 6. Independent Testing Is Mandatory

Each layer must be independently testable.

This is a hard framework rule, not a convenience feature.

Required implications:

- `module` tests cannot depend on a full station existing
- `station` tests cannot depend on a full line existing
- `line` tests cannot replace module or station regression

Each layer should have its own:

- source entry
- scenario set
- intent contract
- verification evidence

## 7. Authoring Tree vs Compile Surface

RustPLC currently compiles through a source entry that closes over:

- `topology`
- `constraints`
- `tasks`

Therefore the preferred near-term architecture is:

1. maintain a layered authoring tree by delivery asset
2. compile through a flattened target-semantics surface

This means:

- authoring structure should follow `module / station / line`
- compile surface may still use `topology/`, `constraints/`, `architecture/`, `auto/`, `faults/`, and related fragment groups
- flattening must preserve ownership in file naming and document references

Do not confuse the compile surface with the full authored architecture.

## 8. Recommended Repository Shape

```text
plc/
  deliveries/
    module/
      pick_head/
        docs/
          module.system.md
          module.architecture.md
          module.intent_alignment.contract.json
          module.verification.md
        plc/
          main.bundle.toml
          target_semantics_fragments/
        scenarios/
          nominal/
    station/
      load_station/
        docs/
          station.system.md
          station.architecture.md
          station.intent_alignment.contract.json
          station.verification.md
        plc/
          main.bundle.toml
          target_semantics_fragments/
        scenarios/
          nominal/
    line/
      wafer_line/
        docs/
          line.system.md
          line.architecture.md
          line.intent_alignment.contract.json
          line.verification.md
        plc/
          main.bundle.toml
          target_semantics_fragments/
        scenarios/
          nominal/
```

## 9. Recommended Compile Surface Naming

When a flattened fragment surface is needed, preserve ownership in file names.

Examples:

- `topology/module_pick_head_devices.plcfrag`
- `topology/station_load_devices.plcfrag`
- `topology/line_layout.plcfrag`
- `auto/station_load_cycle.plcfrag`
- `faults/module_pick_head_faults.plcfrag`
- `architecture/line_supervisor.plcfrag`

This keeps the compileable surface reviewable without erasing the original delivery boundary.

## 10. Contract Direction

Composition must flow upward through explicit contracts:

- a line consumes station contracts
- a station consumes module contracts

Upper layers must not reach down and re-author lower-layer internals.

Examples of forbidden drift:

- line logic depending on a module-internal step name
- station logic depending on an undeclared module-local variable
- line intent contract binding directly to lower-layer implementation details when a station-level milestone exists

## 11. Workpiece Semantics Across Layers

Workpiece semantics remain mandatory wherever real part flow exists.

Layer split:

- module: local handling semantics only if the module really owns a part transition
- station: station-local ingress, process, egress, and abnormal egress
- line: inter-station handoff, buffers, and full-route lifecycle

Do not push line-level workpiece routing down into modules.

Do not omit station-level workpiece semantics just because a line-level model also exists.

## 12. Scaffold Direction

`rust_plc new --layout structured-fragments` is a useful compile-surface scaffold, but it is not yet the full delivery-layer framework.

The target direction is:

- scaffold can start from `module`, `station`, or `line`
- each scaffold includes required architecture documents
- each scaffold includes an independently runnable scenario and intent contract sidecar

Until that exists, skills and human authors should create the delivery-layer document set explicitly instead of treating the current scaffold as sufficient architecture.

## 13. Acceptance Rules

A delivery asset is considered architecturally complete only when:

- its delivery layer is explicit: `module`, `station`, or `line`
- its `*.architecture.md` exists
- its source entry exists
- its scenarios exist
- its intent contract exists or a blocker is explicitly recorded
- it can be independently validated at its own layer

Anything less may compile, but it is not a complete delivery asset.
