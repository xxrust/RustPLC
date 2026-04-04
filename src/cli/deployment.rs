use crate::cli::shared::compile_pipeline::{
    RuntimeBudgetSummary, compile_pipeline, write_verification_report,
};
use crate::cli_support::common::{CliOutputMode, DispatchResult, display_path_relative_to_cwd};
use crate::cli_support::diagnostics_common::evidence_source_label;
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::{
    compile_plc_to_runtime_program, format_loaded_plc_errors,
    parse_loaded_plc_with_required_purpose,
};
use crate::cli_support::runtime_probe::{io_sizes_for_program_and_scenario, is_halted};
use crate::cli_support::scenario_yaml::{
    format_resolve_scenario_yaml_error, parse_scenario_yaml, read_scenario_yaml_file,
    scenario_mismatch_hint_for_example,
};
use io_traits::Io;
use runtime_core::{Action, Instr, Program};
use rust_plc::diagnostics::{DiagnosisInput, EvidenceSource, diagnose};
use rust_plc::io_map::{IoMap, IoMapError, IoUsage};
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::scenario_resolve::resolve_scenario_yaml_for_plc;
use rust_plc::semantic::preprocess_program;
use rust_plc::source_bundle::{LoadedPlcSource, is_supported_plc_source_path, load_plc_source};
use rust_plc::tick_timing::{TickTimingSample, parse_tick_timing_jsonl, to_tick_timing_jsonl};
use rust_plc::timing_report::build_timing_report;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use time::format_description::well_known::Rfc3339;

pub(super) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let (error_prefix, result) = match command {
        "build-rp2040" => (
            Some("[BLD-000]"),
            run_build_rp2040_subcommand(program, remaining.iter().cloned()),
        ),
        "release-bundle" => (
            None,
            run_release_bundle_subcommand(program, remaining.iter().cloned()),
        ),
        "flash-rp2040" => (
            None,
            run_flash_rp2040_subcommand(program, remaining.iter().cloned()),
        ),
        "board-parse" => (
            None,
            run_board_parse_subcommand(program, remaining.iter().cloned()),
        ),
        "no-board-gate" => (
            Some("[GATE-000]"),
            run_no_board_gate_subcommand(program, remaining.iter().cloned()),
        ),
        "commissioning-run" => (
            None,
            run_commissioning_run_subcommand(program, remaining.iter().cloned()),
        ),
        "pil-run" => (
            None,
            run_pil_run_subcommand(program, remaining.iter().cloned()),
        ),
        "virtual-board" => (
            None,
            run_virtual_board_subcommand(program, remaining.iter().cloned()),
        ),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix,
        result,
    })
}

include!("deployment_build.rs");
include!("deployment_release.rs");
include!("deployment_commissioning.rs");
include!("deployment_gate.rs");
include!("deployment_virtual_board.rs");