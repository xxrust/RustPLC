# Project Layout

- `rustplc.project.toml`: project manifest
- `plc/main.system.md`: root system/index document
- `plc/main.target_semantics.bundle.toml`: aggregate compile surface
- `plc/deliveries/`: delivery-layer assets
- `plc/target_semantics_fragments/topology/`: controller, devices, relations, resources
- `plc/target_semantics_fragments/constraints/`: safety and timing rules
- `plc/target_semantics_fragments/architecture/`: startup and supervision
- `plc/target_semantics_fragments/auto/`: automatic production tasks
- `plc/target_semantics_fragments/maintenance/`: maintenance tasks and self-check sidecars
- `plc/target_semantics_fragments/manual/`: manual-mode sidecars
- `plc/target_semantics_fragments/operator_interface/`: operator interface sidecars
- `plc/target_semantics_fragments/io/`: semantic I/O alias sidecars
- `plc/target_semantics_fragments/optimization/`: optimization policy sidecars
- `plc/target_semantics_fragments/step/`: step-mode sidecars
- `plc/target_semantics_fragments/faults/`: warning and fault tasks
- `config/workpiece.toml`: project workpiece policy
- `config/`: deployment and retain configuration
- `out/`: generated artifacts

Current project: `plc_gen_megapipeline` / `Plc Gen Megapipeline`
Current source layout: `bundle + semantic fragments`
Default delivery layer: `line`
