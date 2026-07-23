# Fix Obvious Errors

This public artifact answers one question:

> When a user asks `plc-gen` to `fix` an existing PLC source set, what must the agent check before syntax cleanup or scenario polishing?

## Skill Command

`fix` is an agent repair command, not a `rust_plc` binary subcommand.

When the user says `fix`, the agent must:

1. Read the source entry, scenario, recent diagnostics, and the smallest relevant fragments.
2. Trace each state used to leave a step back to its proof source.
3. Flag unproven success assumptions before changing files.
4. Repair the smallest concrete semantic error or report a blocker.
5. Rerun the relevant validation command and report the real result.

## Sensor-Proof Rule

Production state is sensor-backed unless the system contract explicitly says the step is no-feedback.

Do not assume any physical behavior succeeded by default.

Acceptable proof sources:

- field sensor or controller input
- topology-closed semantic action completion
- workpiece token transition
- operator front-door event
- explicitly documented no-feedback step

Hard failures:

- internal booleans initialized to `true` to pass production waits
- waits on flags that are not derived from sensors, operator commands, workpiece state, or no-feedback semantics
- treating `ingress_sites` as cassette inventory or infinite supply
- continuing a normal path after a physical action without sensor proof, topology-closed completion, timeout/fault routing, or no-feedback documentation

## Example Smell

```plc
variable feed_cassette_has_seed: bool = true

step wait_cassette_source:
    wait: feed_cassette_has_seed == true
    allow_indefinite_wait: true
```

This is not a valid proof that the cassette has a wafer.
It is a test seed or assumption unless it is tied to a cassette sensor, wafer-present sensor, upstream handoff, operator loading action, or explicit no-feedback contract.

The repair is not "set the flag true again."
The repair is to add the missing proof path, block/fault on missing proof, or report a blocker.

## Expected Fix Output

A correct `fix` response should identify:

- the exact unproven state
- where it is initialized or computed
- the step that consumes it
- why the proof source is missing
- the minimum repair direction
- the validation command rerun or the reason it remains blocked
