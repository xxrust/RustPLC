# Scenario Assetization: Coverage + Feedback

Date: 2026-02-19

## Highlights

- Added template library contract at `scenarios/templates/metadata.json`.
- `scenario-gen` now supports:
  - `--coverage-mode pairwise|boundary-first|risk-first`
  - `--dry-run`
  - `--template-library`
- `sim-regress --minimize-failure` now emits `feedback.json`.

## Fast Commands

```bash
cargo run --release -- scenario-gen \
  --plc examples/assembly_station.plc \
  --config examples/scenario_gen/basic.yaml \
  --out-dir out/scenario_gen \
  --coverage-mode risk-first \
  --dry-run
```

```bash
cargo run --release -- sim-regress \
  --plc-dir examples \
  --scenario-dir scenarios \
  --artifacts-dir out/sim-regress \
  --minimize-failure
```

`out/sim-regress/feedback.json` contains template and parameter hints per failure.
