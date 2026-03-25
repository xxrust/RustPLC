# RustPLC Workflow

Use this flow when the caller wants RustPLC to behave like a product they can operate.

## 1. Pick the Launch Mode First

Use one of these command prefixes:

- installed binary mode: `rust_plc`
- source workspace mode: `cargo run --release --bin rust_plc --`

If the environment is unclear, establish the launcher before giving a command sequence.
Do not assume the caller has repository access.

## 2. Pick the Delivery Mode

Use a scaffolded project when the request is any of these:

- a new machine or station
- an end-to-end sample
- a customer handoff project
- scenario validation or no-board gate work
- a request that mentions project layout, files, folders, or delivery artifacts

Use a single-file path only when the user clearly wants one narrow artifact:

- repair an existing `.plc`
- validate one `.plc`
- explain one compiler error

## 3. Recommended Product Flow

For new work, follow this order:

1. Scaffold the project.
2. Fill `plc/main.system.md` with the confirmed system contract.
3. Fill `plc/main.plc` with executable RustPLC DSL.
4. Fill or adjust `scenarios/nominal/normal.yaml`.
5. Run `scenario-validate`.
6. Run `scenario-doctor`.
7. Run `no-board-gate` for project-level acceptance.
8. Run `gen-st` only when ST output is requested.
9. Run `build-rp2040` or `release-bundle` only when deployment artifacts are requested.

Repair in place if validation fails.
Do not stop after generation.

## 4. Blocking Questions Only

Treat these as real blockers:

- start mode
- cycle mode
- key actuator and sensor availability
- whether a wait is indefinite or timed
- timeout and fault routing expectation
- whether independent work should be concurrent tasks

Treat these as conservative-default items unless the user cares:

- placeholder I/O names
- neutral device names
- starter timeout values
- nominal scenario timing values

## 5. Concurrency and Waiting

Model RustPLC according to the existing product semantics:

- tasks may run concurrently
- blocking steps block only their own task
- `wait`, `delay`, `timeout`, and motion waits are blocking by default
- if one station must keep running while another waits, split them into separate tasks

Do not flatten independent work into one long sequential task just because it is easier to explain.

## 6. Validation Standard

Use real commands, not verbal claims.

Minimum validation for a scaffolded project:

1. `scenario-validate`
2. `scenario-doctor`

Preferred project acceptance:

1. `scenario-validate`
2. `scenario-doctor`
3. `no-board-gate`

If ST delivery is requested, also run `gen-st`.
If hardware delivery is requested, also run `build-rp2040` or `release-bundle`.

## 7. Internal Composition Boundary

You may internally think in two stages:

1. system contract
2. PLC generation

But do not force the caller to learn internal repo skill boundaries.
Return one coherent RustPLC delivery package.
