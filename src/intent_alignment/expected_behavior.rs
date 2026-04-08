use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::contract::{
    IntentContract, IntentContractValidationError, IntentMilestone, IntentPostcondition,
    ObservationBinding, RequiredMilestoneEdge, validate_intent_contract,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedBehaviorIrPrimitiveKind {
    StateMachineOrdering,
    ConstraintPostcondition,
    CycleBoundary,
    Restartability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedMilestoneSemanticRole {
    CycleStart,
    RequiredStep,
    CycleComplete,
    Restartable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedMilestoneIrNode {
    pub milestone_id: String,
    pub semantic_roles: Vec<ExpectedMilestoneSemanticRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedBehaviorIrEdge {
    pub predecessor: String,
    pub successor: String,
    pub primitive: ExpectedBehaviorIrPrimitiveKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedPostconditionIrView {
    pub postcondition_id: String,
    pub primitive: ExpectedBehaviorIrPrimitiveKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedMilestoneGraphView {
    pub nodes: Vec<ExpectedMilestoneIrNode>,
    pub edges: Vec<ExpectedBehaviorIrEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedCycleHandoffIrView {
    pub cycle_start_milestone: String,
    pub cycle_complete_milestone: String,
    pub restartable_milestone: String,
    pub next_cycle_start_milestone: String,
    pub required_postconditions: Vec<String>,
    pub cycle_boundary_primitive: ExpectedBehaviorIrPrimitiveKind,
    pub restartability_primitive: ExpectedBehaviorIrPrimitiveKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedBehaviorIrView {
    pub milestone_graph: ExpectedMilestoneGraphView,
    pub postcondition_obligations: Vec<ExpectedPostconditionIrView>,
    pub cycle_handoff: ExpectedCycleHandoffIrView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedRestartability {
    pub restartable_milestone: String,
    pub next_cycle_start_milestone: String,
    pub required_postconditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedCycleSemantics {
    pub cycle_start_milestone: String,
    pub cycle_complete_milestone: String,
    pub restartability: ExpectedRestartability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedBehaviorSpec {
    pub contract_id: String,
    pub contract_version: String,
    pub expected_milestones: Vec<IntentMilestone>,
    pub required_edges: Vec<RequiredMilestoneEdge>,
    pub postconditions: Vec<IntentPostcondition>,
    pub observation_bindings: Vec<ObservationBinding>,
    pub cycle_semantics: ExpectedCycleSemantics,
    pub ir_view: ExpectedBehaviorIrView,
}

#[derive(Debug, Error)]
pub enum ExpectedBehaviorCompileError {
    #[error(transparent)]
    InvalidContract(#[from] IntentContractValidationError),
}

pub fn compile_expected_behavior_spec(
    contract: &IntentContract,
) -> Result<ExpectedBehaviorSpec, ExpectedBehaviorCompileError> {
    validate_intent_contract(contract)?;

    let cycle_semantics = ExpectedCycleSemantics {
        cycle_start_milestone: contract
            .contract_core
            .cycle_semantics
            .cycle_start_milestone
            .clone(),
        cycle_complete_milestone: contract
            .contract_core
            .cycle_semantics
            .cycle_complete_milestone
            .clone(),
        restartability: ExpectedRestartability {
            restartable_milestone: contract
                .contract_core
                .cycle_semantics
                .restart_semantics
                .restartable_milestone
                .clone(),
            next_cycle_start_milestone: contract
                .contract_core
                .cycle_semantics
                .restart_semantics
                .next_cycle_start_milestone
                .clone(),
            required_postconditions: contract
                .contract_core
                .cycle_semantics
                .restart_semantics
                .required_postconditions
                .clone(),
        },
    };

    let milestone_graph = ExpectedMilestoneGraphView {
        nodes: contract
            .contract_core
            .expected_milestones
            .iter()
            .map(|milestone| ExpectedMilestoneIrNode {
                milestone_id: milestone.milestone_id.clone(),
                semantic_roles: semantic_roles_for_milestone(contract, &milestone.milestone_id),
            })
            .collect(),
        edges: contract
            .contract_core
            .required_edges
            .iter()
            .map(|edge| ExpectedBehaviorIrEdge {
                predecessor: edge.predecessor.clone(),
                successor: edge.successor.clone(),
                primitive: ExpectedBehaviorIrPrimitiveKind::StateMachineOrdering,
            })
            .collect(),
    };

    let postcondition_obligations = contract
        .contract_core
        .postconditions
        .iter()
        .map(|postcondition| ExpectedPostconditionIrView {
            postcondition_id: postcondition.postcondition_id.clone(),
            primitive: ExpectedBehaviorIrPrimitiveKind::ConstraintPostcondition,
        })
        .collect();

    let cycle_handoff = ExpectedCycleHandoffIrView {
        cycle_start_milestone: cycle_semantics.cycle_start_milestone.clone(),
        cycle_complete_milestone: cycle_semantics.cycle_complete_milestone.clone(),
        restartable_milestone: cycle_semantics.restartability.restartable_milestone.clone(),
        next_cycle_start_milestone: cycle_semantics
            .restartability
            .next_cycle_start_milestone
            .clone(),
        required_postconditions: cycle_semantics
            .restartability
            .required_postconditions
            .clone(),
        cycle_boundary_primitive: ExpectedBehaviorIrPrimitiveKind::CycleBoundary,
        restartability_primitive: ExpectedBehaviorIrPrimitiveKind::Restartability,
    };

    Ok(ExpectedBehaviorSpec {
        contract_id: contract.metadata.contract_id.clone(),
        contract_version: contract.contract_version.clone(),
        expected_milestones: contract.contract_core.expected_milestones.clone(),
        required_edges: contract.contract_core.required_edges.clone(),
        postconditions: contract.contract_core.postconditions.clone(),
        observation_bindings: contract.observation_bindings.clone(),
        cycle_semantics,
        ir_view: ExpectedBehaviorIrView {
            milestone_graph,
            postcondition_obligations,
            cycle_handoff,
        },
    })
}

fn semantic_roles_for_milestone(
    contract: &IntentContract,
    milestone_id: &str,
) -> Vec<ExpectedMilestoneSemanticRole> {
    let cycle_semantics = &contract.contract_core.cycle_semantics;
    let restart_semantics = &cycle_semantics.restart_semantics;
    let mut roles = vec![ExpectedMilestoneSemanticRole::RequiredStep];

    if milestone_id == cycle_semantics.cycle_start_milestone {
        roles.push(ExpectedMilestoneSemanticRole::CycleStart);
    }
    if milestone_id == cycle_semantics.cycle_complete_milestone {
        roles.push(ExpectedMilestoneSemanticRole::CycleComplete);
    }
    if milestone_id == restart_semantics.restartable_milestone {
        roles.push(ExpectedMilestoneSemanticRole::Restartable);
    }

    roles
}
