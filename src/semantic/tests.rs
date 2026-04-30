#[cfg(test)]
mod tests {
    use super::{
        build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
        preprocess_program, preprocess_program_with_library, validate_source_topology_semantics,
    };
    use crate::device_library::DeviceLibrary;
    use crate::ir::{
        ActionKind, ConnectionType, DeviceKind, SafetyRelation, TaskBlockingState,
        TimerOperationKind, TimingRelation, TimingScope, TransitionGuard,
    };
    use crate::parser::parse_plc;
    use petgraph::visit::EdgeRef;
    use std::path::Path;

    include!("semantic_tests_preprocess.rs");
    include!("semantic_tests_topology.rs");
    include!("semantic_tests_constraints.rs");
    include!("semantic_tests_state_machine.rs");
    include!("semantic_tests_advanced.rs");
}
