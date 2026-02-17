# Scenario Minimization & Feedback Loop

Goal: when a batch regression fails, automatically produce a **small, reproducible** scenario and turn it into an actionable update to templates or generators.

## 1) Run batch regress with minimization

```bash
cargo run --release -- sim-regress \
  --plc-dir examples \
  --scenario-dir scenarios \
  --artifacts-dir out/sim-regress \
  --summary-out out/sim-regress/summary.json \
  --minimize-failure
```

Artifacts (per failing case under `out/sim-regress/case_XXXX/`):
- `trace.jsonl` / `report.json`: original run
- `minimized_scenario.yaml`: minimized repro (includes header comments: source paths + failure signature + feedback)
- `minimized_trace.jsonl` / `minimized_report.json`: minimized run

## 2) Reproduce locally

Copy/paste from `minimized_scenario.yaml` header, or run directly:

```bash
cargo run --release -- sim-plc <file.plc> \
  --scenario out/sim-regress/case_0000/minimized_scenario.yaml \
  --out out/minimized.trace.jsonl
```

If the scenario was authored with `pulse/hold` sugar, the minimized output is the expanded numeric-ID form.
Use `scenario-expand` on the original scenario if you want to inspect/adjust sugar-level intent:

```bash
cargo run --release -- scenario-expand <file.plc> \
  --scenario <original_scenario.yaml> --out out/original.expanded.yaml
```

## 3) Feed back into templates or generators

Common loop for improving "scenario authoring UX":

1. **Template (scenario-init) feedback**
   - If failures often look like "timeout waiting for X", update your template preset to script the missing input edges earlier.
   - For faults, prefer keeping one representative injection (e.g. a single `sensor_stuck`) and make it easy to toggle on/off.

2. **Generator (scenario-gen) feedback**
   - If many minimized cases only differ by `duration_ms`, add a tighter duration grid (or reduce `max_cases`) to keep regression runtime stable.
   - If failures depend on edge ordering, tune `start_pulse_ms` / `sensor_window_ms` windows to create realistic sequencing rather than same-tick satisfaction.

3. **Promote minimized scenario to a named regression**
   - If a minimized case represents a real bug/edge condition, check it in under a stable location (e.g. `scenarios/regress/<name>.yaml`)
   - Wire it into CI via `sim-regress`/`no-board-gate` in your pipeline.
