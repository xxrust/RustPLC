# S02 Cell Loading & Alignment Architecture

## Roles
- Composes preload clamps, vision alignment table, skid drive, and safety release.
- Work positions:
  1. `tray_intake` with gate actuator cylinders C01/C02 and intake motor M01.
  2. `module_preload` using cylinders C03/C04 and motor M02 to seat cells.
  3. `alignment_table` featuring cylinders C05/C06 plus servo drive M03.
  4. `vision_alignment_lane` using cylinders C07/C08, motors M04/M05, and vision stage M06.
  5. `exit_transfer` with cylinders C09/C10 and motors M07/M08 for delivering aligned stacks.

## Actuator inventory
- Cylinders:
  - `C01/C02`: intake lift/clamp
  - `C03/C04`: preload clamp actuators
  - `C05/C06`: alignment table jacks
  - `C07/C08`: vision lane focus
  - `C09/C10`: exit transfer push/pull
- Motors:
  - `M01`: tray intake belt
  - `M02`: skid drive
  - `M03`: servo aligner
  - `M04`: vision rail
  - `M05`: vision stage
  - `M06`: sensor sweep
  - `M07`: exit belt
  - `M08`: data curtain
  - `M09`: coolant pump

## Interfaces
- Input: `alignment_ready` location from S01.
- Output: `cell_loaded` location and explicit `effect: transfer` semantics to the downstream station.
- Fault domain: `fault.reject_alignment` ensures misaligned trays go to `alignment_reject`.
