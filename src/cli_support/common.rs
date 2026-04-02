use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

pub(crate) fn display_path_relative_to_cwd(path: &Path) -> String {
    match env::current_dir() {
        Ok(cwd) => path
            .strip_prefix(&cwd)
            .map(|rel| rel.display().to_string())
            .unwrap_or_else(|_| path.display().to_string()),
        Err(_) => path.display().to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CliOutputMode {
    Human,
    Json,
}

impl CliOutputMode {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "human" => Some(Self::Human),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Json => "json",
        }
    }
}

pub(crate) fn write_jsonl<T: Serialize>(
    path: &Path,
    rows: impl Iterator<Item = T>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create output directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }
    let mut file = fs::File::create(path)
        .map_err(|err| format!("Failed to create output file {}: {err}", path.display()))?;
    for row in rows {
        let line = serde_json::to_string(&row).map_err(|err| {
            format!(
                "Failed to serialize JSONL row for {}: {err}",
                path.display()
            )
        })?;
        writeln!(file, "{line}")
            .map_err(|err| format!("Failed to write {}: {err}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn write_json_pretty<T: Serialize>(path: &Path, payload: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create output directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }
    let mut body = serde_json::to_string_pretty(payload).map_err(|err| {
        format!(
            "Failed to serialize JSON payload for {}: {err}",
            path.display()
        )
    })?;
    body.push('\n');
    fs::write(path, body).map_err(|err| format!("Failed to write {}: {err}", path.display()))
}

pub(crate) struct DispatchResult {
    pub(crate) error_prefix: Option<&'static str>,
    pub(crate) result: Result<(), String>,
}
