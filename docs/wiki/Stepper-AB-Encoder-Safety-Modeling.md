# Stepper + AB Encoder Safety Modeling (Draft)

Date: 2026-02-18

This is a repo-local Wiki draft meant to be readable offline.

Source of truth:
- `docs/stepper_ab_encoder.md` (this draft should stay terminology-compatible with it)
- Scenario workflow: `docs/scenario_playbook.md`

## Scope

This note targets the common industrial setup:

- Actuator: Pulse/Dir stepper axis (STEP/DIR/EN).
- Feedback: incremental AB encoder (high-rate counting/dir inference done in the driver/board layer).
- RustPLC DSL is used for: sequencing, interlocks, wait conditions, and verifiable safety constraints.

## Core Principle: Layering

Keep a strict split:

- DSL (verifiable): only the state machine + interlocks + safety.
- Driver/board (real-time): pulse generation, AB decoding, counting, filtering, unit conversion.
- Feed results back into DSL as simple signals (digital/analog) to keep verification tractable.

## Canonical Safety Abstraction: `zone_code`

`zone_code` is the recommended “collision window encoding” signal:

- Modeled in topology as `analog_input { external: true }`.
- Semantics: `0 = safe`, `1..N = collision window`.
- Produced by the driver/board layer (with hysteresis / LUT / geometry logic); consumed by DSL.

Why this helps:
- Keeps complex geometry/window logic out of the DSL.
- Produces stable, reviewable `safety:` rules.
- Changes to windows become configuration changes in the driver layer, not DSL rewrites.

## Bi-directional Interlock (Minimum Combo)

Do not model collision avoidance as a single one-way rule. The recommended minimum is:

1. Window-side interlock (state-side): when `zone_code != 0`, forbid the actuator dangerous posture.
2. Command-side interlock (command-side): when a motion command is issued, also forbid the actuator dangerous posture (or require “safe posture”).

### Copyable DSL Template (Parseable)

```plc
[topology]
device zone_code: analog_input { range: 0..3, unit: "zone", external: true } # 0=safe, 1..N=collision window
device move_cmd: digital_output
device cyl_clamp: cylinder

[constraints]
# Window-side interlock: collision window forbids dangerous posture.
safety: zone_code > 0 conflicts_with cyl_clamp.extended

# Command-side interlock: motion commands are only legal in safe posture.
safety: move_cmd.on conflicts_with cyl_clamp.extended

[tasks]
task cycle:
    step hold:
```

## Anti-patterns (Common Pitfalls)

- Only writing the window-side interlock (`zone_code > 0 conflicts_with ...`) and forgetting the command-side interlock.
- Encoding windows directly in DSL with multiple thresholds (hard to maintain; hard to express hysteresis; easy to get wrong).
- Exposing raw AB A/B edges to DSL (should be decoded into `count/speed/dir` first).

## Regression Coverage (SIL)

For stable safety modeling, pair the rule set with scenario regression:

- 1 normal case (safe -> move -> stop/inpos).
- At least 1 “count stuck” timeout case.
- At least 1 “wrong direction / bad sign” case.
- At least 1 alarm-triggered fault case.

See `docs/scenario_playbook.md` for scenario authoring + `scenario-validate` + `sim-plc` + batch regression.

