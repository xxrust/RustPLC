# S05 Leak / Hipot / Vision System

## Identity
- Station slug: `s05_leak_hipot_vision`
- Delivery layer: `station`
- Line-level parent: `plc_gen_megapipeline`

## Workpiece and Flow Intent
- The station processes the `battery_module_pack` workpiece coming from S04.
- It enforces electronic continuity validation (leak test), hipot excitation, and vision validation before handing the module to S06.
- Workpiece semantics:
  - `workpiece battery_module_pack: workpiece_type` owns `normal_terminal_states = [validated]` and enters `ingress_sites = [s05_infeed]` / `normal_egress_sites = [s05_outfeed]`.
  - `relationship` to upstream carrier ensures the module is mounted before any high-voltage stimulus.
  - All steps that change module state use `effect: acquire/transfer/finish` so the runtime models the carrier-to-station handoff and release explicitly.

## Process Outline
1. `position_load` receives the carrier and clamps the module with the leak shield clamps (cylinders 1‑4).
2. `position_seal` drives the hipot tank servo motors (motors 1‑4) and electric clamps (cylinders 5‑6) to seal the chamber.
3. `position_hipot` energizes the hipot source via servomotors 5‑6 and monitors `sensor_hipot_ready`.
4. `position_vision` rotates the module with motors 7‑8 and inspects it with machine-vision sensors, then releases.

## Delivery-Grade Expectations
- The station must stay independently simulatable: no downstream steps or handwritten sensor choreography should be required to simulate leak/hipot/vision.
- Configuration documents will specify tolerances, failure routes, and rework criteria that belong to this station alone.
