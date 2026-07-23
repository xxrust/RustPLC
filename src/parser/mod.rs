use crate::ast::{
    ActionStatement, ActionTarget, AxisAutoResetPolicy, AxisFaultContractDeclaration,
    AxisFaultPropagationScope, AxisFaultRouteDirective, AxisFaultRouteKind, AxisFaultSeverity,
    AxisStopMode, BinaryOperator, Branch, CamPoint, CamTableDeclaration, CamTableMode,
    CausalityConstraint, ComparisonOperator, ConditionExpression, ConstraintsSection,
    ControllerIoAliasDeclaration, ControllerIoDeclaration, ControllerIoDirection,
    ControllerSyncDeclaration, DeviceAttributes, DeviceDeclaration, DeviceInstanceDeclaration,
    DevicePort, DeviceTags, DeviceTemplateDeclaration, DeviceType, DurationValue, EffectKind,
    EffectStatement, Expression, ExternCallBinding, ExternFunctionContract,
    ExternFunctionDeclaration, ExternFunctionParameter, GotoDirective, LiteralValue, MeasuredValue,
    OnCompleteDirective, ParallelBlock, PlcProgram, PortRole, PortType, RaceBlock, RaceBranch,
    ResourceClaimConstraint, ResourceClaimSource, SafetyConstraint, SafetyOperand, SafetyRelation,
    SemanticResourceDeclaration, SemanticResourceMode, StateReference, StepDeclaration,
    StepStatement, TaskDeclaration, TaskInstanceDeclaration, TaskTemplateDeclaration, TasksSection,
    TemplateDeviceDeclaration, TemplateDeviceType, TimeUnit, TimeoutDirective, TimingConstraint,
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
    reject_excessive_source_nesting(input)?;
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
        if let Some(col_idx) = deprecated_connected_to_column(line) {
            return Err(PlcError::parse_at(
                "<input>",
                line_idx + 1,
                col_idx + 1,
                "属性 connected_to 已废弃，请改用 relation { from: Device.Port, to: Device.Port, via: ... }",
            ));
        }
    }

    Ok(())
}

fn deprecated_connected_to_column(line: &str) -> Option<usize> {
    const KEYWORD: &str = "connected_to";
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if byte == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if byte == b'#' {
            break;
        }
        if bytes.get(index..index.saturating_add(KEYWORD.len())) == Some(KEYWORD.as_bytes()) {
            let has_identifier_prefix = index > 0
                && matches!(bytes[index - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_');
            let mut tail_index = index + KEYWORD.len();
            while matches!(bytes.get(tail_index), Some(b' ' | b'\t')) {
                tail_index += 1;
            }
            if !has_identifier_prefix && bytes.get(tail_index) == Some(&b':') {
                return Some(index);
            }
        }
        index += 1;
    }

    None
}

fn parse_finite_f64(raw: &str, line: usize, context: &str) -> Result<f64, PlcError> {
    let value = raw.parse::<f64>().map_err(|_| {
        PlcError::parse(line, format!("{context} numeric literal is invalid: {raw}"))
    })?;
    if !value.is_finite() {
        return Err(PlcError::parse_with_reason(
            line,
            format!("{context} numeric literal must be finite: {raw}"),
            "use a finite numeric value within the supported range",
        ));
    }
    Ok(value)
}

fn reject_excessive_source_nesting(input: &str) -> Result<(), PlcError> {
    const MAX_SOURCE_PAREN_DEPTH: usize = 128;
    let mut depth = 0usize;
    let mut unary_minus_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (line_index, line) in input.lines().enumerate() {
        for byte in line.bytes() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }

            match byte {
                b'"' => in_string = true,
                b'#' => break,
                b'-' => {
                    unary_minus_depth = unary_minus_depth.saturating_add(1);
                    if unary_minus_depth > MAX_SOURCE_PAREN_DEPTH {
                        return Err(PlcError::parse_with_reason(
                            line_index + 1,
                            format!(
                                "expression unary depth exceeds limit {MAX_SOURCE_PAREN_DEPTH}"
                            ),
                            "split deeply nested expressions into intermediate compute statements",
                        ));
                    }
                }
                b'(' => {
                    unary_minus_depth = 0;
                    depth = depth.saturating_add(1);
                    if depth > MAX_SOURCE_PAREN_DEPTH {
                        return Err(PlcError::parse_with_reason(
                            line_index + 1,
                            format!(
                                "expression/source nesting depth exceeds limit {MAX_SOURCE_PAREN_DEPTH}"
                            ),
                            "split deeply nested expressions into intermediate compute statements",
                        ));
                    }
                }
                b')' => {
                    unary_minus_depth = 0;
                    depth = depth.saturating_sub(1);
                }
                byte if byte.is_ascii_whitespace() => {}
                _ => unary_minus_depth = 0,
            }
        }
    }

    Ok(())
}

include!("topology.rs");
include!("constraints.rs");
include!("tasks.rs");
include!("tests.rs");
