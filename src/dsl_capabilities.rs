use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DslCapabilitiesReport {
    pub schema_version: u32,
    pub command: &'static str,
    pub output: String,
    pub parser_contract: &'static str,
    pub supported_features: Vec<DslCapabilityEntry>,
    pub template_assets: Vec<DslTemplateAsset>,
    pub unsupported_features: Vec<DslUnsupportedFeature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DslCapabilityEntry {
    pub id: &'static str,
    pub status: &'static str,
    pub layer: &'static str,
    pub summary: &'static str,
    pub evidence: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DslTemplateAsset {
    pub id: &'static str,
    pub status: &'static str,
    pub summary: &'static str,
    pub paths: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DslUnsupportedFeature {
    pub id: &'static str,
    pub status: &'static str,
    pub reason: &'static str,
    pub required_contract: &'static str,
}

pub fn build_dsl_capabilities_report(output: impl Into<String>) -> DslCapabilitiesReport {
    DslCapabilitiesReport {
        schema_version: 1,
        command: "dsl-capabilities",
        output: output.into(),
        parser_contract: "Only grammar-backed DSL constructs are accepted as source semantics; asset templates are external authoring aids and do not imply generic DSL expansion.",
        supported_features: vec![
            DslCapabilityEntry {
                id: "device_profile_config_refs",
                status: "supported",
                layer: "semantic",
                summary: "Devices can bind model_ref/config_ref/motion_param_set assets and are resolved before IR lowering.",
                evidence: vec![
                    "src/axis_profile.rs",
                    "devices/controllers/openplc_softplc.toml",
                    "devices/axis_models/stepper_generic.toml",
                ],
            },
            DslCapabilityEntry {
                id: "station_protocols",
                status: "supported",
                layer: "semantic_ir_verification",
                summary: "station, handshake, transfer_point, controller_sync, and controller inventory declarations are validated, retained in TopologyGraph.station_protocol, and reported by verification.",
                evidence: vec![
                    "src/parser/plc.pest",
                    "src/ast/mod.rs",
                    "src/semantic/semantic_core.rs",
                    "src/ir/mod.rs",
                    "src/verification/mod.rs",
                    "tests/verification_report.rs",
                ],
            },
            DslCapabilityEntry {
                id: "generic_device_templates",
                status: "supported",
                layer: "preprocess_semantic_ir",
                summary: "device_template and device_instance support type-parameterized device generation; instances expand before IR so verification, runtime, and codegen consume ordinary devices.",
                evidence: vec![
                    "src/parser/plc.pest",
                    "src/ast/mod.rs",
                    "src/semantic/preprocess.rs",
                    "src/semantic/semantic_tests_preprocess.rs",
                    "docs/architecture/generic-device-templates.md",
                ],
            },
            DslCapabilityEntry {
                id: "generic_task_templates",
                status: "supported",
                layer: "preprocess_semantic_ir",
                summary: "task_template and task_instance support identifier-parameterized task generation; instances expand before IR so verification, runtime, and codegen consume ordinary tasks.",
                evidence: vec![
                    "src/parser/plc.pest",
                    "src/ast/mod.rs",
                    "src/semantic/preprocess.rs",
                    "src/semantic/semantic_tests_preprocess.rs",
                    "docs/architecture/generic-task-templates.md",
                ],
            },
            DslCapabilityEntry {
                id: "advanced_pattern_matching",
                status: "supported",
                layer: "parser_semantic_ir",
                summary: "match supports one explicit case plus default as grammar-backed pattern-branch sugar and lowers to existing IfElse semantics before IR.",
                evidence: vec![
                    "src/parser/plc.pest",
                    "src/parser/tasks.rs",
                    "src/parser/tests.rs",
                    "docs/architecture/pattern-match-sugar.md",
                ],
            },
            DslCapabilityEntry {
                id: "workpiece_flow_semantics",
                status: "supported",
                layer: "verification_runtime",
                summary: "Workpiece type, site, holder, carrier, split, merge, mount, and transfer semantics lower into IR constraints and runtime effects.",
                evidence: vec![
                    "src/semantic/semantic_workpiece_lowering.rs",
                    "src/verification/safety_workpiece.rs",
                    "crates/runtime-core/src/lib.rs",
                ],
            },
            DslCapabilityEntry {
                id: "extern_function_contracts",
                status: "supported",
                layer: "semantic_ir",
                summary: "Extern functions declare typed signatures, purity, and time bounds and lower into state-machine actions.",
                evidence: vec![
                    "src/parser/plc.pest",
                    "src/semantic/semantic_externs.rs",
                    "src/extern_functions.rs",
                ],
            },
        ],
        template_assets: vec![
            DslTemplateAsset {
                id: "recovery_templates",
                status: "asset_template",
                summary: "Recovery examples are catalogued PLC source assets, not generic DSL template declarations.",
                paths: vec![
                    "examples/recovery_templates/estop_recovery.plc",
                    "examples/recovery_templates/power_loss_recovery.plc",
                    "examples/recovery_templates/sensor_stuck_recovery.plc",
                    "examples/catalog.toml",
                ],
            },
            DslTemplateAsset {
                id: "scenario_templates",
                status: "asset_template",
                summary: "Scenario generation templates are external scenario assets consumed by scenario-gen.",
                paths: vec!["scenarios/templates/metadata.json", "src/cli/scenario.rs"],
            },
            DslTemplateAsset {
                id: "deployment_io_templates",
                status: "asset_template",
                summary: "Board deployment commands emit fill-in I/O map and analog calibration templates.",
                paths: vec![
                    "src/cli/deployment_build.rs",
                    "src/cli/deployment_release.rs",
                ],
            },
        ],
        unsupported_features: vec![DslUnsupportedFeature {
            id: "macro_expansion",
            status: "unsupported",
            reason: "RustPLC treats source DSL as explicit control semantics; untracked macro expansion would weaken diagnostics and verification traceability.",
            required_contract: "Define hygienic expansion artifacts, source maps, deterministic ordering, and tests across parser, LSP, IR, verification, runtime, and codegen.",
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::build_dsl_capabilities_report;

    #[test]
    fn report_keeps_supported_and_unsupported_boundaries() {
        let report = build_dsl_capabilities_report("json");

        assert_eq!(report.schema_version, 1);
        assert!(report.supported_features.iter().any(|feature| {
            feature.id == "station_protocols" && feature.layer == "semantic_ir_verification"
        }));
        assert!(report.supported_features.iter().any(|feature| {
            feature.id == "generic_device_templates" && feature.layer == "preprocess_semantic_ir"
        }));
        assert!(report.supported_features.iter().any(|feature| {
            feature.id == "generic_task_templates" && feature.layer == "preprocess_semantic_ir"
        }));
        assert!(report.supported_features.iter().any(|feature| {
            feature.id == "advanced_pattern_matching" && feature.layer == "parser_semantic_ir"
        }));
        assert!(
            report.template_assets.iter().any(|asset| {
                asset.id == "recovery_templates" && asset.status == "asset_template"
            })
        );
        assert!(
            !report
                .unsupported_features
                .iter()
                .any(|feature| feature.id == "generic_device_templates")
        );
        assert!(
            !report
                .unsupported_features
                .iter()
                .any(|feature| feature.id == "generic_task_templates")
        );
        assert!(
            !report
                .unsupported_features
                .iter()
                .any(|feature| feature.id == "advanced_pattern_matching")
        );
    }
}
