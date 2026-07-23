# Axis Move Blocking Baseline System Contract

## Purpose

This module proves the runtime contract for a blocking relative axis move without
an additional hand-written sensor wait. The step may advance only after the axis
action reports `Done`; timeout and three distinct fault classes route to explicit
fault steps.

## Sequence

1. Request a relative move of 20 units at speed 2.
2. Poll the pending action across ticks.
3. Route timeout, reject, motion fault, and safety fault to separate handlers.
4. Emit `axis move done` only after successful completion.

## Verification Obligations

- Pending must not replay preceding immediate side effects.
- The success route must remain blocked until the runtime action reports `Done`.
- Every declared fault route must resolve to an executable step.
- Formal verification and no-board execution do not prove target drive wiring,
  motion tuning, physical travel limits, or target-hardware timing.

## Delivery Boundary

Compiler, runtime-bridge, scenario-schema, and no-board evidence are automated.
Axis wiring, travel-limit validation, drive commissioning, HIL timing, and release
approval remain attributable human holds.
