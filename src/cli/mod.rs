pub(crate) mod component;
pub(crate) mod diagnostics;
pub(crate) mod scenario;
pub(crate) mod utilities;

use rust_plc::source_bundle::is_supported_plc_source_path;
use std::env;
use std::path::Path;
use crate::cli_support::help;

pub(crate) fn run() {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "rust_plc".to_string());

    let Some(first) = args.next() else {
        help::print_usage(&program);
        std::process::exit(1);
    };

    let remaining: Vec<String> = args.collect();

    if help::is_help_flag(&first) {
        help::print_usage(&program);
        std::process::exit(0);
    }

    if first == "help" {
        match remaining.as_slice() {
            [] => {
                help::print_usage(&program);
                std::process::exit(0);
            }
            [command] if help::is_help_flag(command) => {
                help::print_usage(&program);
                std::process::exit(0);
            }
            [command] => help::print_command_help_and_exit(&program, command, 0),
            _ => {
                eprintln!("{}", help::command_usage(&program, "help"));
                std::process::exit(1);
            }
        }
    }

    if help::help_requested_for_invocation(&first, &remaining) {
        if help::cli_command_help(&first).is_some() {
            help::print_command_help_and_exit(&program, first.as_str(), 0);
        }
        if is_supported_plc_source_path(Path::new(&first)) {
            help::print_command_help_and_exit(&program, "compile", 0);
        }
        eprintln!("Unknown command: {first}");
        help::print_usage(&program);
        std::process::exit(1);
    }

    if first == "sim" {
        if let Err(msg) = crate::run_sim_subcommand(&program, remaining.clone().into_iter()) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "sim-regress" {
        if let Err(msg) = crate::run_sim_regress_subcommand(&program, remaining.clone().into_iter())
        {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "sim-pid-kpi" {
        if let Err(msg) = crate::run_sim_pid_kpi_subcommand(&program, remaining.clone().into_iter())
        {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "sim-plc" {
        if let Err(msg) = crate::run_sim_plc_subcommand(&program, remaining.clone().into_iter()) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "build-rp2040" {
        if let Err(msg) =
            crate::run_build_rp2040_subcommand(&program, remaining.clone().into_iter())
        {
            eprintln!("[BLD-000] {msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "release-bundle" {
        if let Err(msg) =
            crate::run_release_bundle_subcommand(&program, remaining.clone().into_iter())
        {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "flash-rp2040" {
        if let Err(msg) =
            crate::run_flash_rp2040_subcommand(&program, remaining.clone().into_iter())
        {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "board-parse" {
        if let Err(msg) = crate::run_board_parse_subcommand(&program, remaining.clone().into_iter())
        {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = diagnostics::try_dispatch(&program, &first, &remaining) {
        if let Err(msg) = result.result {
            if let Some(prefix) = result.error_prefix {
                eprintln!("{prefix} {msg}");
            } else {
                eprintln!("{msg}");
            }
            std::process::exit(1);
        }
        return;
    }
    if let Some(component_result) = component::try_dispatch(&program, &first, &remaining) {
        if let Err(msg) = component_result.result {
            if let Some(prefix) = component_result.error_prefix {
                eprintln!("{prefix} {msg}");
            } else {
                eprintln!("{msg}");
            }
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = utilities::try_dispatch(&program, &first, &remaining) {
        if let Err(msg) = result.result {
            if let Some(prefix) = result.error_prefix {
                eprintln!("{prefix} {msg}");
            } else {
                eprintln!("{msg}");
            }
            std::process::exit(1);
        }
        return;
    }
    if first == "no-board-gate" {
        if let Err(msg) =
            crate::run_no_board_gate_subcommand(&program, remaining.clone().into_iter())
        {
            eprintln!("[GATE-000] {msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "commissioning-run" {
        if let Err(msg) =
            crate::run_commissioning_run_subcommand(&program, remaining.clone().into_iter())
        {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "pil-run" {
        if let Err(msg) = crate::run_pil_run_subcommand(&program, remaining.clone().into_iter()) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "virtual-board" {
        if let Err(msg) =
            crate::run_virtual_board_subcommand(&program, remaining.clone().into_iter())
        {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = scenario::try_dispatch(&program, &first, &remaining) {
        if let Err(msg) = result.result {
            if let Some(prefix) = result.error_prefix {
                eprintln!("{prefix} {msg}");
            } else {
                eprintln!("{msg}");
            }
            std::process::exit(1);
        }
        return;
    }
    if first == "new" {
        if let Err(msg) = crate::run_new_subcommand(&program, remaining.clone().into_iter()) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }

    crate::run_compile_command(program, first, remaining);
}
