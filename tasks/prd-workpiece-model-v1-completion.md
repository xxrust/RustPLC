# PRD: Workpiece Model V1 Completion

## Introduction

`docs/workpiece_model_spec.md` defines the WPM-v1 semantic boundary, but the repository is still in a partial state:

- Parser / Semantic / IR support exists for much of Phase 1-3
- Verification is still partly approximate for split / merge and reachable-state contract checks
- `runtime_bridge` still rejects workpiece programs as a whole
- `runtime-core` does not yet execute workpiece effects

This PRD scopes the remaining WPM-v1 work into Ralph-sized stories that close the semantic, verification, runtime bridge, runtime-core, and example-regression path.

## Goals

- Remove blanket runtime rejection for supported WPM-v1 workpiece flows
- Add bounded runtime token execution for Phase 1-3 workpiece effects
- Upgrade workpiece verification from declaration-only / type-count checks to instance-aware reachable-state checks
- Build a stable long-term regression gate from the existing workpiece examples and tests

## User Stories

### US-001: Add bounded workpiece token storage to runtime-core
**Description:** As a runtime maintainer, I need a bounded token store so runtime execution can track workpiece instance location, lifecycle, and occupancy without dynamic allocation.

**Acceptance Criteria:**
- [ ] Add bounded workpiece token storage in `crates/runtime-core/src/lib.rs`
- [ ] Token storage tracks token id, workpiece type, current location, active state, and terminal status
- [ ] Storage remains `no_std` compatible and does not use heap allocation
- [ ] Add runtime-core tests for token creation, movement, termination, and capacity limits
- [ ] Typecheck passes
- [ ] Tests pass

### US-002: Lower Phase 1 workpiece resources and effects in runtime_bridge
**Description:** As a compiler maintainer, I want `state_machine_to_runtime_program` to lower Phase 1 workpiece flows instead of returning `WorkpieceModelUnsupported`.

**Acceptance Criteria:**
- [ ] `src/runtime_bridge.rs` no longer rejects programs that only use Phase 1 workpiece features
- [ ] Bridge lowers workpiece types, workpiece sites, holders, and Phase 1 effects into runtime metadata
- [ ] Programs using unsupported later-phase workpiece effects still fail with effect-level bridge errors
- [ ] Add bridge regression coverage for the Phase 1 workpiece example
- [ ] Typecheck passes
- [ ] Tests pass

### US-003: Execute Phase 1 acquire / transfer / finish in runtime-core
**Description:** As a PLC author, I want Phase 1 workpiece flows to execute in runtime so site-to-holder-to-site motion and finish semantics are real, not just IR.

**Acceptance Criteria:**
- [ ] Runtime executes `acquire`, `transfer`, and `finish`
- [ ] `acquire` moves a token from a site into a holder
- [ ] `transfer` updates token location across holder, site, and slot endpoints
- [ ] `finish` removes the token from occupancy and records terminal completion
- [ ] Runtime returns explicit errors for underflow, duplicate occupancy, and overflow
- [ ] Add runtime tests for successful flow and failure paths
- [ ] Typecheck passes
- [ ] Tests pass

### US-004: Verify Phase 1 ingress / egress / endpoint uniqueness on reachable states
**Description:** As a verification maintainer, I need exact reachable-state checks for ingress, egress, terminal bucket, and endpoint uniqueness violations.

**Acceptance Criteria:**
- [ ] Safety verification checks that tokens only enter through declared `ingress_sites`
- [ ] `finish` only exits through the correct normal or abnormal egress bucket
- [ ] Safety verification catches duplicate occupancy on reachable states, not just by preflight estimation
- [ ] Add negative verification tests for illegal ingress, wrong egress bucket, duplicate occupancy, and dangling tokens
- [ ] Keep the public verification summary shape stable
- [ ] Typecheck passes
- [ ] Tests pass

### US-005: Lower Phase 2 carrier / slot / mount / unmount / transform in runtime_bridge
**Description:** As a compiler maintainer, I want Phase 2 carrier semantics to reach runtime instead of stopping at IR.

**Acceptance Criteria:**
- [ ] Bridge lowers `workpiece_carrier`, slot addresses, `mount`, `unmount`, and `transform carrier`
- [ ] Runtime program metadata preserves carrier layout, slot capacity, and transform frame data
- [ ] Invalid slot references and undeclared carriers return explicit bridge errors
- [ ] Add bridge regression coverage for `examples/workpiece_carrier_slot_transfer.plc`
- [ ] Typecheck passes
- [ ] Tests pass

### US-006: Execute Phase 2 mount / unmount / transform in runtime-core
**Description:** As a PLC author, I want mounted workpieces to be tracked in carrier slots and remain consistent across carrier transforms.

**Acceptance Criteria:**
- [ ] Runtime executes `mount`, `unmount`, and `transform carrier`
- [ ] Slot occupancy is updated correctly on mount and unmount
- [ ] Carrier transform keeps mounted token association intact
- [ ] Runtime returns explicit errors for empty-slot unmount, slot overflow, and duplicate mount
- [ ] Add runtime tests for success and failure paths
- [ ] Typecheck passes
- [ ] Tests pass

### US-007: Verify Phase 2 carrier consistency and slot capacity on reachable states
**Description:** As a verification maintainer, I need exact reachable-state checks for carrier-mounted token consistency.

**Acceptance Criteria:**
- [ ] Safety verification rejects states where a token is both free-standing and mounted at the same time
- [ ] Safety verification rejects slot overflow and out-of-bounds slot occupancy
- [ ] Carrier transform preserves mounted token consistency in the reachable-state model
- [ ] Add negative verification coverage for slot overflow and inconsistent mounted state
- [ ] Typecheck passes
- [ ] Tests pass

### US-008: Add lineage data structures for split / merge
**Description:** As a system maintainer, I need explicit lineage storage so split and merge can be modeled as instance-level semantics rather than only type-level declarations.

**Acceptance Criteria:**
- [ ] Add lineage metadata needed to represent parent-to-child and merge-input-to-output relations
- [ ] Runtime lineage storage is bounded and `no_std` compatible
- [ ] Consumed tokens remain traceable through lineage records
- [ ] Add focused tests for lineage creation and lookup
- [ ] Typecheck passes
- [ ] Tests pass

### US-009: Lower and execute split
**Description:** As a PLC author, I want `split ... count N consumed` to create N output tokens and consume the source token in runtime.

**Acceptance Criteria:**
- [ ] Bridge lowers split source type, target type, count, and consumed metadata
- [ ] Runtime split creates the requested number of output tokens
- [ ] Runtime split records parent lineage for every produced token
- [ ] Runtime split consumes the source token when `consumed` is set
- [ ] Add bridge and runtime regressions for valid split and split failure paths
- [ ] Typecheck passes
- [ ] Tests pass

### US-010: Lower and execute merge
**Description:** As a PLC author, I want `merge [a, b] into x consumed_inputs` to consume input tokens and create an output token in runtime.

**Acceptance Criteria:**
- [ ] Bridge lowers merge input references, target type, and consumed-input metadata
- [ ] Runtime merge consumes input tokens and creates the output token
- [ ] Runtime merge records merge lineage
- [ ] Runtime returns explicit errors for missing inputs, duplicate consumed input, and arity mismatch
- [ ] Add bridge and runtime regressions for valid merge and merge failure paths
- [ ] Typecheck passes
- [ ] Tests pass

### US-011: Verify instance-level split / merge legality
**Description:** As a verification maintainer, I need split and merge checks to run on instance lineage, not only on type counts.

**Acceptance Criteria:**
- [ ] Safety verification rejects split outputs that do not come from a valid source token
- [ ] Safety verification rejects merge outputs that do not come from the declared legal input set
- [ ] Safety verification rejects repeated consumption of the same merge input token
- [ ] Terminal-state checks use instance token state, not only type-count approximations
- [ ] Add negative verification coverage for illegal lineage and repeated input consumption
- [ ] Typecheck passes
- [ ] Tests pass

### US-012: Expand the workpiece example regression gate
**Description:** As a maintainer, I want the existing workpiece examples to become the stable regression gate for compile, runtime bridge, runtime behavior, and verification.

**Acceptance Criteria:**
- [ ] Keep `examples/workpiece_phase1_transfer.plc`, `examples/workpiece_carrier_slot_transfer.plc`, and `examples/workpiece_split_merge.plc` as the canonical workpiece fixtures
- [ ] `tests/examples_integration.rs` covers compile-to-json expectations for all three examples
- [ ] Runtime bridge and runtime behavior regressions reuse the same examples where practical
- [ ] Verification regressions reuse or mirror the same example semantics where practical
- [ ] Typecheck passes
- [ ] Tests pass

## Functional Requirements

1. FR-1: `runtime_bridge` must stop rejecting all workpiece programs as a blanket rule.
2. FR-2: `runtime-core` must execute WPM-v1 workpiece semantics using bounded token storage.
3. FR-3: Phase 1 effects must preserve endpoint uniqueness, capacity, and terminal completion rules.
4. FR-4: Phase 2 effects must preserve carrier-slot consistency.
5. FR-5: Phase 3 effects must preserve lineage and consumed-input semantics.
6. FR-6: Safety verification must reject clearly illegal reachable-state workpiece behavior.
7. FR-7: Existing example fixtures must become the long-term regression gate for workpiece semantics.

## Non-Goals

- No ST backend work in this PRD
- No Phase 4 classify, rework, recirculation, or return-flow modeling
- No continuous geometry or unbounded token graphs
- No runtime or verification support for semantics outside WPM-v1 Phase 1-3

## Technical Considerations

- `crates/runtime-core` is `no_std`, so workpiece state must use bounded structures
- Keep the public verification summary shape stable
- Prefer reusing `tests/workpiece_model_phase23.rs`, `tests/examples_integration.rs`, and `examples/workpiece_*.plc`
- Avoid silent semantic fallback in bridge or runtime; unsupported cases must fail explicitly

## Success Metrics

- The three workpiece examples compile successfully
- Phase 1 and Phase 2 examples lower into runtime programs successfully
- Runtime regressions execute workpiece flows instead of rejecting them
- Verification catches instance-level split / merge errors that are currently only approximated
- `cargo test --test workpiece_model_phase23`, `cargo test --test examples_integration`, and `cargo test --lib` pass
