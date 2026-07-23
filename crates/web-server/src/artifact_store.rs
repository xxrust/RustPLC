use std::path::{Component, Path, PathBuf};

pub(crate) fn is_safe_relative_path(raw: &str) -> bool {
    if raw.is_empty() || raw.len() > 4096 || raw.contains(['\\', '%', '\0']) || raw.starts_with('/')
    {
        return false;
    }
    let path = Path::new(raw);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(crate) fn resolve_workspace_input(workspace_root: &Path, raw: &str) -> Result<PathBuf, String> {
    if !is_safe_relative_path(raw) {
        return Err("path must be a normalized workspace-relative path".to_string());
    }
    let root = workspace_root
        .canonicalize()
        .map_err(|_| "workspace root is unavailable".to_string())?;
    let path = root.join(raw);
    let canonical = path
        .canonicalize()
        .map_err(|_| "input file does not exist".to_string())?;
    if !canonical.starts_with(&root) {
        return Err("input path escapes the workspace".to_string());
    }
    let metadata = std::fs::metadata(&canonical)
        .map_err(|_| "input file metadata is unavailable".to_string())?;
    if !metadata.is_file() {
        return Err("input path must reference a file".to_string());
    }
    if metadata.len() > crate::MAX_INPUT_FILE_BYTES {
        return Err("input file exceeds the web execution limit".to_string());
    }
    Ok(canonical)
}

pub(crate) fn resolve_workspace_output_path(
    workspace_root: &Path,
    relative: &str,
) -> Result<PathBuf, String> {
    if !is_safe_relative_path(relative) {
        return Err("path must be a normalized workspace-relative path".to_string());
    }
    let root = workspace_root
        .canonicalize()
        .map_err(|_| "workspace root is unavailable".to_string())?;
    let candidate = root.join(relative);
    let mut existing = candidate.as_path();
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| "output path is unavailable".to_string())?;
    }
    let existing = existing
        .canonicalize()
        .map_err(|_| "output path is unavailable".to_string())?;
    if !existing.starts_with(&root) {
        return Err("output path escapes the workspace".to_string());
    }
    if candidate.exists() {
        let canonical = candidate
            .canonicalize()
            .map_err(|_| "output path is unavailable".to_string())?;
        if !canonical.starts_with(&root) {
            return Err("output path escapes the workspace".to_string());
        }
    }
    Ok(candidate)
}

pub(crate) fn workspace_output_root(workspace_root: &Path) -> Result<PathBuf, String> {
    let root = workspace_root
        .canonicalize()
        .map_err(|_| "workspace root is unavailable".to_string())?;
    let out = root.join("out");
    std::fs::create_dir_all(&out)
        .map_err(|_| "workspace output directory is unavailable".to_string())?;
    let out = out
        .canonicalize()
        .map_err(|_| "workspace output directory is unavailable".to_string())?;
    if !out.starts_with(&root) {
        return Err("workspace output directory escapes the workspace".to_string());
    }
    Ok(out)
}

pub(crate) fn artifact_href(workspace_root: &Path, path: &Path) -> String {
    let output_root =
        workspace_output_root(workspace_root).unwrap_or_else(|_| workspace_root.join("out"));
    let rel = path
        .strip_prefix(output_root)
        .map(|part| part.to_string_lossy().replace('\\', "/"));
    match rel {
        Ok(rel) => format!("/artifacts/{rel}"),
        Err(_) => "/artifacts/invalid".to_string(),
    }
}

pub(crate) fn artifact_href_any(workspace_root: &Path, raw_path: &str) -> Option<String> {
    let path = resolve_internal_artifact_path(workspace_root, raw_path)?;
    Some(artifact_href(workspace_root, &path))
}

pub(crate) fn resolve_artifact_reference(
    workspace_root: &Path,
    reference: &str,
) -> Option<PathBuf> {
    let rel = reference.strip_prefix("/artifacts/").unwrap_or(reference);
    if !is_safe_relative_path(rel) {
        return None;
    }
    let lexical = workspace_root.join("out").join(rel);
    if !lexical.exists() {
        return Some(lexical);
    }
    let out_root = workspace_output_root(workspace_root).ok()?;
    let path = lexical.canonicalize().ok()?;
    path.starts_with(&out_root).then_some(path)
}

fn resolve_internal_artifact_path(workspace_root: &Path, raw: &str) -> Option<PathBuf> {
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path.canonicalize().ok()?
    } else {
        workspace_root.join(path).canonicalize().ok()?
    };
    let out_root = workspace_output_root(workspace_root).ok()?;
    path.starts_with(&out_root).then_some(path)
}

pub(crate) fn read_text_file_limited(path: &Path, max_bytes: u64) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|err| err.to_string())?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err("file is unavailable or exceeds the size limit".to_string());
    }
    std::fs::read_to_string(path).map_err(|err| err.to_string())
}
