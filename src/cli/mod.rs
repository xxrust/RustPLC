mod compile;
mod component;
mod deployment;
mod diagnostics;
mod project;
mod scenario;
mod shared;
mod sim;
mod utilities;

use crate::cli_support::help;
use std::env;

pub(super) fn run() {
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
            [command] if help::cli_command_help(command).is_some() => {
                help::print_command_help_and_exit(&program, command, 0)
            }
            [_] => help::print_command_help_and_exit(&program, "compile", 0),
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
        help::print_command_help_and_exit(&program, "compile", 0);
    }

    if let Some(result) = sim::try_dispatch(&program, &first, &remaining) {
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
    if let Some(result) = deployment::try_dispatch(&program, &first, &remaining) {
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
    if let Some(result) = project::try_dispatch(&program, &first, &remaining) {
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

    compile::run_compile_command(program, first, remaining);
}
