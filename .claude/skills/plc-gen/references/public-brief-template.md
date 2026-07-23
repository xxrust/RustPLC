# plc-gen Public Brief Template

Use this brief before delegating complex `plc-gen` work.

## Required Sections

### Task Goal
- what the user wants delivered
- whether this is generation, repair, restructuring, or project delivery
- what counts as success

### Current Source Shape
- single-file `.plc`
- or `.bundle.toml` plus fragments
- whether a scaffolded project already exists

### Frozen Lowering Facts
- confirmed task partition
- blocking and timeout behavior
- topology-closed device-action assumptions
- workpiece-flow requirements
- mode, supervisor, warning, and fault structure

### Existing Files
- authoritative intent source
- current source entry
- scenario path
- current sidecar path if it exists

### Authored Artifacts For This Round
- files the implementer may edit
- files the implementer must not edit
- whether the round must create or repair `*.intent_alignment.contract.json`

For project-scale delivery, the default answer here is `yes`.

### Validation Requirement
- the exact validation command
- whether `intent_alignment` must appear in `project-check`
- what verdict is required for acceptance

### Blockers And Assumptions
- current blockers
- current assumptions
- what would invalidate the chosen source shape
