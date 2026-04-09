# S04 Laser Weld & Cooling Station

## Identity
- Station slug: `s04_laser_weld_cooling`
- Delivery layer: `station`
- Finishes the tab weld and applies controlled cooling before handing off to S05.

## Process Intent
1. Receive tab-prepped busbars from S03 and transfer them into the dual-laser work envelope.
2. Engage the clamp array (8 cylinders) and spin up the filter-cooled servo-laser gantry (10 motors) to execute `weld_tabs`.
3. After welding, move the carrier to the cooling chute using the servo conveyor while the cooling fan array ramps down gradually to the `cooling_ready` semantic result.
4. Transfer the cooled assembly to the buffer for S05 leak/hipot inspection.

## Work Positions
- Laser window load/unload (position 1)
- Clamp array with eight pneumatic cylinders (positions 2-3) + interlock
- Laser gantry and servo stage (position 4)
- Cooling fan manifold and exit conveyor (position 5)
- Transfer buffer toward S05 (position 6)
- Operator review gate for weld verification (position 7)

## Workpiece Semantics
- Workpiece `battery_module_pack` continues along the same line-level part identity declared upstream.
- Locations: `s04_infeed`, `laser_chamber`, `cooling_run`, `s05_buffer`, each capacity 1 or 2 for buffering.
- Semantic effects: `acquire holder weld_head from s04_infeed`, `effect: transfer` through each location, `effect: finish workpiece at s05_buffer as weld_cooled`.

## Actuators
- 8 welding clamp cylinders, 2 transfer cylinders.
- 10 motors: dual laser scan stage servos, gantry traverse servo, cooling conveyor servos, inspection turntable, fan speed motor, buffer servo.

## Fault Strategy
- Laser action exposes `on_motion_fault: goto fault.weld_fault`.
- Cooling step ensures `cooling_ready` milestone before transfer; otherwise park the carrier and flag a manual reset.
