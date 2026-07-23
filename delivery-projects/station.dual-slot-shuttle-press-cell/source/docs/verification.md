# Verification Plan

## Required Gates

- compile/verify structured fragments;
- state-proof check without fabricated success flags;
- process-model refinement without OP-002/OP-003 being hidden;
- sequence lint and scenario doctor;
- nominal `sim-plc` trace covering seven business milestones;
- sibling intent contract with real SHA-256 and trace-backed anchors;
- `project-check` containing both `process_model_check` and `intent_alignment`.

## Safety Claims

- shuttle motion and press extension never overlap `shuttle_envelope`;
- load/unload manipulation is serialized by `load_station_access`;
- blocking axis/cylinder actions expose complete fault routing;
- finite two-token ingress cannot trigger a third acquire;
- every task-driven actuator is self-checked or has a documented visible-output exemption.
