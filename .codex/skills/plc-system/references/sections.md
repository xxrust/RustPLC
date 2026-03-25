# plc-system Sections

Use this file when drafting `main.system.md`.

## Always Include

- project identity
- system mission
- safety and reliability level
- operating environment
- normal process flow
- abnormal handling
- concurrent task partition
- blocking step expectations
- startup and stop flow
- testing and maintenance modes
- key constraints
- AI generation guidance

## Add When Motion Exists

- parameter layering
- homing and soft limits
- fault policy
- propagation scope

## Blocking Semantics

The system document must state:

- which activities should become separate tasks
- which waits are blocking steps
- which tasks must continue while another task is blocked
- which resources are shared or mutually exclusive
