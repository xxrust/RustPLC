use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tracing::{error, info};

use crate::{AppState, RustPlcLauncher};

#[derive(Debug)]
pub(crate) struct CommandOutput {
    pub(crate) success: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) async fn run_rust_plc(
    state: &Arc<AppState>,
    args: &[String],
) -> Result<CommandOutput, String> {
    let mut command = match &state.rust_plc_launcher {
        RustPlcLauncher::Binary(bin_path) => {
            info!("run configured rust_plc binary {:?}", bin_path);
            let mut command = Command::new(bin_path);
            command.args(args);
            command
        }
        RustPlcLauncher::Cargo => {
            info!("run rust_plc through cargo");
            let mut cargo_args = vec![
                "run".to_string(),
                "--quiet".to_string(),
                "--bin".to_string(),
                "rust_plc".to_string(),
                "--".to_string(),
            ];
            cargo_args.extend(args.iter().cloned());
            let mut command = Command::new("cargo");
            command.args(cargo_args);
            command
        }
    };
    command
        .current_dir(&state.workspace_root)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to start rust_plc command: {err}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to capture rust_plc stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture rust_plc stderr".to_string())?;
    let (status, stdout, stderr) = tokio::time::timeout(state.run_timeout, async move {
        tokio::join!(
            child.wait(),
            read_command_stream(stdout),
            read_command_stream(stderr)
        )
    })
    .await
    .map_err(|_| "rust_plc command timed out and was terminated".to_string())?;
    let status = status.map_err(|err| format!("failed to wait for rust_plc command: {err}"))?;
    let stdout = stdout?;
    let stderr = stderr?;

    Ok(CommandOutput {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    })
}

async fn read_command_stream(mut stream: impl AsyncRead + Unpin) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| "failed to read rust_plc command output".to_string())?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > crate::MAX_COMMAND_OUTPUT_BYTES {
            return Err("rust_plc command output exceeded the web limit".to_string());
        }
        output.extend_from_slice(&chunk[..read]);
    }
}

pub(crate) fn first_failure_message(stderr: &str, stdout: &str) -> String {
    if let Some(line) = stderr.lines().find(|line| !line.trim().is_empty()) {
        return line.trim().to_string();
    }
    if let Some(line) = stdout.lines().find(|line| !line.trim().is_empty()) {
        return line.trim().to_string();
    }
    "command failed without details".to_string()
}

pub(crate) fn public_command_failure(workspace_root: &Path, output: &CommandOutput) -> String {
    let raw = first_failure_message(&output.stderr, &output.stdout);
    error!("rust_plc command failed: {}", raw);
    public_command_error(workspace_root, &raw)
}

pub(crate) fn public_command_error(workspace_root: &Path, raw: &str) -> String {
    let workspace = workspace_root.to_string_lossy();
    let mut message = raw.replace(workspace.as_ref(), "<workspace>");
    if let Ok(canonical) = workspace_root.canonicalize() {
        message = message.replace(canonical.to_string_lossy().as_ref(), "<workspace>");
    }
    message = message
        .lines()
        .next()
        .unwrap_or("command failed")
        .to_string();
    message.truncate(512);
    if message.trim().is_empty() {
        "rust_plc command failed".to_string()
    } else {
        message
    }
}
