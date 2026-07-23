# Three-station Assembly Line System Contract

## Purpose

The line sequences a push station, press station, quality decision, and good/reject
ejection path. Cylinder actions are topology-closed through valve, actuator, and
endpoint sensor relations. A cycle must complete within 10 seconds or route to a
safe retract fault handler.

## Normal Flow

1. Wait for the operator start input.
2. Extend the push cylinder and confirm its extended endpoint.
3. Extend and retract the press cylinder with bounded endpoint waits.
4. Wait for exactly one quality result.
5. Route good parts to the eject cylinder and bad parts to the reject cylinder.
6. Retract the active routing cylinder and push cylinder in parallel.
7. Return to ready for the next cycle.

## Fault Flow

Any bounded actuator or quality wait may enter `fault_handler`. The handler
requests safe retraction of press, push, eject, and reject cylinders, records the
fault, and returns to ready.

## Safety And Timing

- Press extension requires the push cylinder to remain extended.
- The complete cycle has a 10 second timing requirement.
- Declared causality chains bind PLC outputs through valves and cylinders to both
  endpoint sensors.

## Delivery Boundary

The source snapshot currently preserves mojibake in legacy authored comments and
diagnostic strings. The executable identifiers and topology compile, while source
text remediation remains an explicit quality blocker. Physical point checks,
fault-injection observation, target timing, and signed release holds are absent.
