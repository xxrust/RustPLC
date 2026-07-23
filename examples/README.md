# RustPLC Examples

This directory intentionally keeps the historical example paths stable while the
project migrates toward categorized example groups. Many tests, docs, and CLI
help examples still reference the current paths directly.

`catalog.toml` is the machine-readable classification source for this index.
Run `cargo run --bin rust_plc -- examples-index --catalog examples/catalog.toml --output json`
to validate that catalog entries and scenario links still point at real files.

Use this index as the navigation layer before moving files into subdirectories.
Any future physical reorganization should update `catalog.toml`, tests, docs,
CLI help text, and scenario references in the same change.

## 01 Basics

| Example | Purpose |
| --- | --- |
| [`demo.plc`](demo.plc) | Minimal language and device demonstration. |
| [`process_device_demo.plc`](process_device_demo.plc) | Process-device topology and task flow example. |
| [`quadratic_fit.plc`](quadratic_fit.plc) | Compute/extern-style numeric workflow fixture. |

## 02 Motion Control

| Example | Purpose |
| --- | --- |
| [`dual_axis_platform.plc`](dual_axis_platform.plc) | Canonical dual-axis motion example used by quickstart docs. |
| [`rp2040_motion_minimal.plc`](rp2040_motion_minimal.plc) | Board-oriented motion example paired with RP2040 scenarios and IO map. |
| [`stepper_collision_guard.plc`](stepper_collision_guard.plc) | Stepper safety and collision-guard scenario fixture. |
| [`axis_move_blocking_baseline.plc`](axis_move_blocking_baseline.plc) | Blocking axis move baseline without hand-written wait steps. |
| [`axis_servo_fault_routing.plc`](axis_servo_fault_routing.plc) | Servo fault routing fixture. |
| [`axis_stepper_fault_routing.plc`](axis_stepper_fault_routing.plc) | Stepper fault routing fixture. |
| [`axis_fault_normal_path.plc`](axis_fault_normal_path.plc) | Axis normal-path fault policy fixture. |
| [`axis_fault_recoverable_path.plc`](axis_fault_recoverable_path.plc) | Axis recoverable fault policy fixture. |
| [`axis_fault_nonrecoverable_path.plc`](axis_fault_nonrecoverable_path.plc) | Axis nonrecoverable fault policy fixture. |
| [`axis_fault_safety_path.plc`](axis_fault_safety_path.plc) | Axis safety fault policy fixture. |

## 03 Process And Station Flow

| Example | Purpose |
| --- | --- |
| [`three_station_assembly.plc`](three_station_assembly.plc) | Multi-station assembly sequence. |
| [`welding_station.plc`](welding_station.plc) | Welding station sequence and constraints. |
| [`load_unload_concurrent_tasks.plc`](load_unload_concurrent_tasks.plc) | Concurrent load/unload task fixture. |
| [`realtime_stress/stress_case.plc`](realtime_stress/stress_case.plc) | No-board gate and realtime stress playbook fixture. |
| [`project_scaffold_demo/plc/main.plc`](project_scaffold_demo/plc/main.plc) | Structured project scaffold reference used by scenario tools. |

## 04 Workpiece And Material Flow

| Example | Purpose |
| --- | --- |
| [`workpiece_phase1_transfer.plc`](workpiece_phase1_transfer.plc) | Phase 1 acquire/transfer/finish workpiece flow. |
| [`workpiece_carrier_slot_transfer.plc`](workpiece_carrier_slot_transfer.plc) | Carrier slot transfer fixture. |
| [`workpiece_split_merge.plc`](workpiece_split_merge.plc) | Split/merge lineage fixture. |

## 05 Safety, Recovery, And Diagnostics

| Example | Purpose |
| --- | --- |
| [`nuclear_coolant_isolation.plc`](nuclear_coolant_isolation.plc) | High-criticality safety example. |
| [`force_override_demo.plc`](force_override_demo.plc) | Online force, retain, commissioning, and control-plane fixture. |
| [`recovery_templates/estop_recovery.plc`](recovery_templates/estop_recovery.plc) | Emergency-stop recovery template. |
| [`recovery_templates/power_loss_recovery.plc`](recovery_templates/power_loss_recovery.plc) | Power-loss recovery template. |
| [`recovery_templates/sensor_stuck_recovery.plc`](recovery_templates/sensor_stuck_recovery.plc) | Sensor-stuck recovery template. |
| [`error_all_verifiers.plc`](error_all_verifiers.plc) | Negative fixture spanning verification failures. |
| [`error_cam_missing_table.plc`](error_cam_missing_table.plc) | Negative fixture for missing cam data. |
| [`error_missing_device.plc`](error_missing_device.plc) | Negative fixture for missing devices. |

## 06 Performance And Deployment Fixtures

| Example | Purpose |
| --- | --- |
| [`topology_perf_500.plc`](topology_perf_500.plc) | Large topology performance fixture. |
| [`pil_baselines/case_timeout/case.plc`](pil_baselines/case_timeout/case.plc) | PIL/Renode timeout baseline. |

## Reorganization Checklist

Before moving any example file into a categorized directory:

1. Update all test fixtures that reference the old path.
2. Update README, docs, wiki pages, and CLI help examples.
3. Update scenario, IO-map, intent-contract, and release-bundle references.
4. Run `cargo test -p rust_plc --test examples_integration`.
5. Run targeted scenario and deployment tests for moved examples.
