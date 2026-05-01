use crate::ast::{
    ActionStatement, ActionTarget, AxisAutoResetPolicy, AxisFaultContractDeclaration,
    AxisFaultPropagationScope, AxisFaultRouteDirective, AxisFaultRouteKind, AxisFaultSeverity,
    AxisStopMode, BinaryOperator, Branch, CamPoint, CamTableDeclaration, CamTableMode,
    CausalityConstraint, ComparisonOperator, ConditionExpression, ConstraintsSection,
    DeviceAttributes, DeviceDeclaration, DevicePort, DeviceTags, DeviceType, DurationValue,
    EffectKind, EffectStatement, Expression, ExternCallBinding, ExternFunctionContract,
    ExternFunctionDeclaration, ExternFunctionParameter, GotoDirective, LiteralValue, MeasuredValue,
    OnCompleteDirective, ParallelBlock, PlcProgram, PortRole, PortType, RaceBlock, RaceBranch,
    ResourceClaimConstraint, ResourceClaimSource, SafetyConstraint, SafetyOperand, SafetyRelation,
    SemanticResourceDeclaration, SemanticResourceMode, StateReference, StepDeclaration,
    StepStatement, TaskDeclaration, TasksSection, TimeUnit, TimeoutDirective, TimingConstraint,
    TimingRelation, TimingTarget, TopologyConnection, TopologyRelation, TopologySection,
    VariableDeclaration, VariableType, WaitCondition, WaitStatement, WorkpieceAllowDeclaration,
    WorkpieceCarrierDeclaration, WorkpieceCarrierLayout, WorkpieceDerivationDeclaration,
    WorkpieceHolderDeclaration, WorkpiecePropertyDeclaration, WorkpiecePropertyType,
    WorkpieceSiteDeclaration, WorkpieceSiteKind, WorkpieceTypeDeclaration,
};
use crate::error::PlcError;
use pest::Parser;
use pest::error::LineColLocation;
use pest::iterators::Pair;
use std::collections::HashSet;

#[derive(pest_derive::Parser)]
#[grammar = "parser/plc.pest"]
pub struct PlcParser;

pub fn parse_topology(input: &str) -> Result<(), pest::error::Error<Rule>> {
    PlcParser::parse(Rule::topology_file, input).map(|_| ())
}

pub fn parse_constraints(input: &str) -> Result<(), pest::error::Error<Rule>> {
    PlcParser::parse(Rule::constraints_file, input).map(|_| ())
}

pub fn parse_tasks(input: &str) -> Result<(), pest::error::Error<Rule>> {
    PlcParser::parse(Rule::tasks_file, input).map(|_| ())
}

pub fn parse_plc(input: &str) -> Result<PlcProgram, PlcError> {
    reject_deprecated_connected_to(input)?;
    let mut pairs = PlcParser::parse(Rule::plc_file, input).map_err(map_parse_error)?;
    let plc_pair = pairs
        .next()
        .ok_or_else(|| PlcError::parse(1, "未找到可解析的 PLC 程序"))?;

    let program = parse_plc_pair(plc_pair)?;
    reject_extern_calls_in_expression_context(&program)?;
    Ok(program)
}

fn reject_deprecated_connected_to(input: &str) -> Result<(), PlcError> {
    for (line_idx, line) in input.lines().enumerate() {
        let code = line.split('#').next().unwrap_or(line);
        if let Some(col_idx) = code.find("connected_to") {
            let tail = &code[col_idx + "connected_to".len()..];
            if tail.trim_start().starts_with(':') {
                return Err(PlcError::parse_at(
                    "<input>",
                    line_idx + 1,
                    col_idx + 1,
                    "属性 connected_to 已废弃，请改用 relation { from: Device.Port, to: Device.Port, via: ... }",
                ));
            }
        }
    }

    Ok(())
}

include!("topology.rs");
include!("constraints.rs");
include!("tasks.rs");
include!("tests.rs");
