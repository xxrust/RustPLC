# PRD: Analog Sensor Derivation & Verification Integrity

## Introduction/Overview

The current verification engine silently skips analog threshold safety rules and treats analog inputs as generic sensors in causality inference. This leads to misleading “complete proof” reports, missing coverage for analog constraints, and false causality errors for external inputs (e.g., operator setpoints or ADC channels). This feature introduces derived sensors from analog inputs, explicit external input marking, and analog-aware verification so safety and wait conditions are handled consistently without silent skips.

## Goals

- Eliminate silent skipping of analog threshold safety rules and avoid misleading “complete proof” reports.
- Support analog threshold comparisons in both Safety constraints and `wait` conditions via discrete abstraction.
- Provide a DSL-level way to model analog-derived sensors (threshold / hysteresis / debounce) for clearer logic and causality.
- Reduce false causality errors for external inputs through explicit `external` marking.
- Add tests and docs to lock behavior and explain limitations.

## User Stories

### US-001: Declare derived sensors from analog inputs
**Description:** As a controls engineer, I want to derive discrete sensors from analog inputs (with threshold/hysteresis/debounce) so that logic and causality remain discrete and verifiable.

**Acceptance Criteria:**
- [ ] DSL supports `sensor` declaration with `source`, `threshold`, optional `hysteresis`, optional `debounce`.
- [ ] Parser and semantic layers accept the new syntax and validate required fields.
- [ ] Derived sensors can be used in `wait` and `safety` rules just like standard sensors.
- [ ] `cargo test` passes.

### US-002: Mark external inputs to skip causality inference
**Description:** As a controls engineer, I want to mark inputs as external so causality inference does not require PLC outputs to “drive” them.

**Acceptance Criteria:**
- [ ] `external: true` is accepted on `digital_input` and `analog_input` devices.
- [ ] Causality inference skips action→sensor inference for external inputs.
- [ ] Non-external inputs retain existing causality checks.
- [ ] `cargo test` passes.

### US-003: Validate analog threshold comparisons
**Description:** As a developer, I want semantic validation for analog threshold comparisons so invalid types or out-of-range thresholds are rejected early.

**Acceptance Criteria:**
- [ ] Threshold comparisons in `safety` and `wait` reject non-analog devices (unless they are derived sensors).
- [ ] Threshold values outside the device `range` produce a semantic error.
- [ ] If an analog device lacks `range`, threshold usage produces a semantic error.
- [ ] `cargo test` passes.

### US-004: Include analog thresholds in Safety verification
**Description:** As a user, I want Safety verification to actually check analog threshold rules (or explicitly warn when it cannot), so the report is not misleading.

**Acceptance Criteria:**
- [ ] Safety verification includes analog threshold rules using a discrete abstraction based on used thresholds.
- [ ] If a rule cannot be modeled, Safety summary includes a warning and the proof level is downgraded (no “complete proof”).
- [ ] Verification output clearly lists skipped/partially modeled analog rules.
- [ ] `cargo test` passes.

### US-005: Handle analog thresholds in `wait` conditions
**Description:** As a user, I want `wait` conditions with analog comparisons to be representable in the state machine so liveness/timing checks are consistent.

**Acceptance Criteria:**
- [ ] `wait` analog comparisons are mapped to discrete predicates derived from threshold partitions.
- [ ] Liveness checks treat analog waits as conditions (not automatically “always true”).
- [ ] Behavior matches documented semantics for analog `wait` (including timeouts/allow_indefinite_wait).
- [ ] `cargo test` passes.

### US-006: Documentation and examples
**Description:** As a user, I want documentation and examples showing derived sensors and external inputs so I can model real systems correctly.

**Acceptance Criteria:**
- [ ] Docs explain: external inputs vs feedback sensors, derived sensor syntax, and verification limitations.
- [ ] At least one example demonstrates analog input + derived sensor usage.
- [ ] `cargo test` passes.

## Functional Requirements

- FR-1: Add `external: true` attribute for `digital_input` and `analog_input` devices (default `false`).
- FR-2: Causality inference must skip action→sensor inference for external inputs.
- FR-3: Introduce derived sensor DSL with `source` (analog_input), `threshold`, optional `hysteresis`, optional `debounce`.
- FR-4: Derived sensors must be usable wherever sensors are used today (wait/safety/causality).
- FR-5: Threshold comparisons in `safety` and `wait` must validate device type (analog or derived sensor) and range.
- FR-6: Analog threshold rules must be included in Safety verification via discrete partitioning of analog domains.
- FR-7: Safety summary must downgrade proof level and emit warnings if any analog rule is not fully modeled.
- FR-8: Analog `wait` conditions must be represented as discrete predicates in the state machine for liveness/timing checks.
- FR-9: Update docs and examples to cover derived sensors and external inputs.

## Non-Goals (Out of Scope)

- Full continuous-time or real-valued verification without abstraction.
- PID control modeling and verification.
- Hardware abstraction layer integration (EtherCAT/Modbus/GPIO).
- SMT-based real arithmetic reasoning (unless explicitly requested later).

## Design Considerations

- Prefer discrete derived sensors for process feedback; keep `analog_input` as raw channels.
- Example DSL (illustrative):
  - `sensor pressure_hi: sensor { source: AI0, threshold: 6.0, hysteresis: 0.2, debounce: 50ms }`
- External inputs represent operator setpoints or external systems; do not require a PLC action chain.

## Technical Considerations

- Discrete abstraction should be generated from all thresholds used in `safety` and `wait` for each analog source.
- For equality (`==`) comparisons, define a consistent mapping (e.g., treat as “in the exact threshold bin” or discourage usage with a warning).
- Proof level semantics: “complete proof” only when all rules are modeled; otherwise “bounded/partial” with explicit warnings.
- Backward compatibility: programs without new fields should behave as before, except for improved warnings/validation.

## Success Metrics

- 0 instances of “complete proof” when analog threshold rules are skipped.
- At least one test covering each of: derived sensor parsing, external input causality skip, analog threshold validation, analog wait modeling.
- Reduced false causality errors for external inputs in sample programs.

## Open Questions

- How should equality (`==`) on analog values be represented in discrete abstraction?
- Should derived sensors be materialized as devices in the IR, or kept as semantic aliases?
- Should external inputs be allowed to participate in causality if explicitly referenced by a causality chain?
