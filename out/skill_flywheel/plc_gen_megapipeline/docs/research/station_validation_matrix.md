# Station Validation Matrix

## Purpose
This matrix records whether each station asset is independently executable, not just documented.

| Station | Initial state | Root issue | Current state |
| --- | --- | --- | --- |
| `s01_tray_infeed_buffer` | Passed `project-check`, but only through shared line fragments | False-positive independence | Passes local station bundle |
| `s02_cell_loading_alignment` | Passed `project-check`, but only through shared line fragments | False-positive independence | Passes local station bundle |
| `s03_busbar_tab_prep` | Passed `project-check`, but only through shared line fragments | False-positive independence | Passes local station bundle |
| `s04_laser_weld_cooling` | Passed `project-check`, but only through shared line fragments | False-positive independence | Passes local station bundle |
| `s05_leak_hipot_vision` | Failed `project-check` | Raw DSL pasted into `main.bundle.toml` | Passes local station bundle |
| `s06_label_packout_sort` | Failed `project-check` | Raw DSL pasted into `main.bundle.toml` | Passes local station bundle |

## Stable Lessons
- A delivery asset only counts as independently testable when its own entry file points at its own executable PLC fragments.
- Shared line fragments are acceptable for line integration, but they are not acceptable proof of station independence.
- For station canaries, representative actuators are enough, but workpiece semantics and high-level actuator actions are mandatory.
