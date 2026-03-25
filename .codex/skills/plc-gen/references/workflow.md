# plc-gen Workflow

Use this file when the caller needs a product-style RustPLC generation flow instead of repo internals.

## 1. Pick the Launch Mode

Use one of these prefixes:

- installed binary mode: `rust_plc`
- source workspace mode: `cargo run --release --bin rust_plc --`

Do not use `cargo run --release -- ...`.
This workspace has multiple binaries.

## 2. Pick the Delivery Shape

Use a scaffolded project when the user wants:

- a new machine or station
- a customer handoff project
- end-to-end validation
- scenario validation or no-board gate
- exact folder and file guidance

Use a single-file flow only when the request is narrow:

- repair one `.plc`
- validate one `.plc`
- explain one compiler or validation failure

## 3. Recommended Generation Path

For a new project:

1. scaffold the project
2. use the scaffolded `scenarios/nominal/normal.yaml` as the starting nominal scenario
3. confirm or write `plc/main.system.md`
4. generate or repair `plc/main.plc`
5. tune `scenarios/nominal/normal.yaml`
5. run `scenario-validate`
6. run `scenario-doctor`
7. run `no-board-gate` when the request is project-level
8. run `gen-st` only when ST output is requested

For an existing PLC:

1. inspect the requirement or current failure
2. repair `main.plc`
3. validate with `scenario-validate`
4. diagnose with `scenario-doctor`
5. export ST if requested

Do not recommend `scenario-init` immediately after `new`.
The scaffold already provides `scenarios/nominal/normal.yaml`.
Use `scenario-init` only when the scenario file is missing or the caller wants to regenerate a scenario skeleton from a standalone `.plc`.

## 4. Blocking Questions Only

Treat these as real blockers:

- start mode
- cycle mode
- key actuator and sensor availability
- whether a wait is indefinite or timed
- timeout and fault routing expectation
- whether independent work should run as separate tasks

Treat these as conservative defaults unless the user cares:

- placeholder I/O names
- neutral device names
- starter timeout values
- nominal scenario timing values

## 5. Concurrency and Blocking

Model RustPLC according to current product semantics:

- tasks may run concurrently
- blocking steps block only their own task
- `wait`, `delay`, `timeout`, and motion waits are blocking by default
- if one station must keep running while another waits, split them into separate tasks

Do not flatten independent work into one sequential task just because it is easier to describe.

## 6. Completion Rule

Generation is complete only when the actual RustPLC tooling passes or a precise contract gap remains.
