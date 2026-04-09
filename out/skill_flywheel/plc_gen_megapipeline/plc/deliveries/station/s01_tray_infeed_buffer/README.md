# S01 Tray Infeed Buffer Station

- Delivery layer: `station`
- Identity: `s01_tray_infeed_buffer`
- Role: choreographs tray-level buffering, clamp readiness, and infeed transfer for the line’s module-pack assembly.
- Work positions: 1) Infeed Gate, 2) Tray Buffer, 3) Clamp Readiness, 4) Transfer Arm Center, 5) Emergency Clamp Release (idle support).
- Actuator inventory: 12 cylinders (infeed gate, buffer pistons, clamp latching, venting) and 8 motors (dual positioning motors, servo-indexer, belt drives).

This station owns its own `docs/*.system.md`, architecture/verification/intent artifacts, PLC bundle, and scenario. The station presents a workpiece entry contract to the downstream cell-loading station and exposes no line-level details beyond that contract.
