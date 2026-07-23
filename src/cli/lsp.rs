pub(super) struct DispatchResult {
    pub(super) result: Result<(), String>,
    pub(super) error_prefix: Option<&'static str>,
}

pub(super) fn try_dispatch(
    program: &str,
    first: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    if first != "lsp" {
        return None;
    }

    if remaining
        .iter()
        .any(|arg| crate::cli_support::help::is_help_flag(arg))
    {
        crate::cli_support::help::print_command_help_and_exit(program, "lsp", 0);
    }

    if !remaining.is_empty() {
        return Some(DispatchResult {
            result: Err(crate::cli_support::help::command_usage(program, "lsp")),
            error_prefix: None,
        });
    }

    Some(DispatchResult {
        result: run_lsp_server(),
        error_prefix: Some("LSP server failed:"),
    })
}

fn run_lsp_server() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|err| format!("failed to start Tokio runtime: {err}"))?;
    runtime.block_on(rust_plc::lsp::run_stdio_server());
    Ok(())
}
