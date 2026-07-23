# 05 Safety, Recovery, And Diagnostics

Safety, recovery template, force override, and negative diagnostic fixtures.

| Example | Kind | Source | Scenario | Purpose |
| --- | --- | --- | --- | --- |
| `nuclear_coolant_isolation` | `plc` | [`examples/nuclear_coolant_isolation.plc`](../../nuclear_coolant_isolation.plc) |  | High-criticality safety example. |
| `force_override_demo` | `plc` | [`examples/force_override_demo.plc`](../../force_override_demo.plc) |  | Online force, retain, commissioning, and control-plane fixture. |
| `estop_recovery` | `template` | [`examples/recovery_templates/estop_recovery.plc`](../../recovery_templates/estop_recovery.plc) |  | Emergency-stop recovery template. |
| `power_loss_recovery` | `template` | [`examples/recovery_templates/power_loss_recovery.plc`](../../recovery_templates/power_loss_recovery.plc) |  | Power-loss recovery template. |
| `sensor_stuck_recovery` | `template` | [`examples/recovery_templates/sensor_stuck_recovery.plc`](../../recovery_templates/sensor_stuck_recovery.plc) |  | Sensor-stuck recovery template. |
| `error_all_verifiers` | `negative_plc` | [`examples/error_all_verifiers.plc`](../../error_all_verifiers.plc) |  | Negative fixture spanning verification failures. |
| `error_cam_missing_table` | `negative_plc` | [`examples/error_cam_missing_table.plc`](../../error_cam_missing_table.plc) |  | Negative fixture for missing cam data. |
| `error_missing_device` | `negative_plc` | [`examples/error_missing_device.plc`](../../error_missing_device.plc) |  | Negative fixture for missing devices. |
