# DualSlot Shuttle Press Cell System Contract

## Identity And Mission

- Delivery layer: station.
- Source entry: `rustplc.bundle.toml`.
- Mission: load exactly two `insert_part` tokens into two concrete shuttle slots, move to press, press, return, unload, finish, and restore ready state.
- Nominal execution is a finite two-part acceptance cycle. A third unseeded acquire is forbidden.

## Safety And Resources

- `shuttle_envelope` is shared by shuttle motion and press-cylinder hazardous action; overlap is forbidden.
- `load_station_access` serializes load and unload workpiece manipulation.
- Shuttle motion requires the topology-closed press cylinder to be retracted.
- Axis timeout and command reject are recoverable after operator reset and safe return.
- Axis motion and safety faults remain maintenance-latched.
- Cylinder timeout or feedback anomaly inhibits motion and requires safe retract proof.

## Topology And Operator Boundary

- Controller `plc_main` uses a controller profile and project-level aliases.
- `axis_shuttle` provides blocking absolute moves with timeout, reject, motion-fault, and safety-fault routes.
- `press_cylinder` is topology-closed; tasks use high-level extend/retract actions and never restate endpoint waits.
- Start and reset are physical push-button field devices reporting to controller inputs; the human is not a device.
- Ready, running, fault, rejection, buzzer, HMI status, and HMI fault code are visible feedback obligations.

## Workpiece And Capacity

- Type: `insert_part`.
- `raw_infeed` capacity 4 and `good_outfeed` capacity 4.
- `load_nest` and `press_nest` capacity 1.
- `shuttle_tray` is a two-slot carrier with concrete `slot[0]` and `slot[1]`, each capacity 1.
- Flow: acquire from `raw_infeed`, mount to a free slot, move to press, transform while mounted, return, unmount, transfer to `good_outfeed`, finish as `finished`.

## Concurrent Tasks

- `startup_self_check`: proves safe cylinder and axis lifecycle before readiness.
- `operator_front_door`: continuously consumes rising-edge start commands, accepts only the first ready/safe command, and visibly rejects before-ready, running, faulted, or completed-batch commands.
- `load_unload`: owns acquire, mount, unmount, transfer, and finish under `load_station_access`.
- `shuttle_motion`: owns blocking axis moves under `shuttle_envelope`.
- `press_process`: owns high-level cylinder press sequence under `shuttle_envelope`.
- `fault_recovery`: classifies recoverable versus maintenance faults, waits for reset where allowed, and performs safe return.
- `manual_assist`: consumes the physical residual summary and operator empty confirmation before startup may continue.
- `supervision` and `hmi_feedback`: publish machine state, PLC-internal numeric status/fault codes, and physical visible/audible outputs.

## Blocking And Proof Rules

- Axis move, cylinder action, delay, and wait block only their owning task.
- Operator start/reset may wait indefinitely with rising-edge semantics.
- Local feedback requires timeout and explicit routing.
- Completed-motion dependent effects live in following steps.
- No internal boolean initialized true may stand in for a physical state.

## Process Intent And Milestones

The required lowering order is `main.system.md -> process_model/process_operation_model.toml -> task/step -> process-model-check`.

Business milestones are: `cycle_accepted`, `slots_loaded`, `arrived_press`, `press_completed`, `returned_load`, `parts_finished`, and `ready_next_cycle`.

In this finite-batch specimen, `ready_next_cycle` is the observable end-of-batch handoff. A new run requires external replenishment and runtime reinitialization; the current program does not implement an in-process second cycle.

Residual workpiece safety combines the audited commissioning baseline in `config/state_proof.toml`, physical `residual_present`, and `operator_empty_confirm`. The executable `manual_assist` path blocks startup until the residual signal clears and the operator confirms the empty baseline. Runtime injection and preservation of an exact pre-existing token location remain a product capability gap.

## Scenarios

Authored scenarios cover startup self-check, nominal two-part cycle, before-ready rejection followed by a valid start, start while running, start while faulted, residual manual assist, axis timeout recovery, axis safety fault, and cylinder timeout.
