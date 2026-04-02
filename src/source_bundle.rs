use crate::error::{PlcError, SourceLocation};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const BUNDLE_SUFFIX: &str = ".bundle.toml";
const SUPPORTED_BUNDLE_SECTIONS: [&str; 3] = ["topology", "constraints", "tasks"];

#[derive(Debug, Clone)]
pub struct LoadedPlcSource {
    pub requested_path: PathBuf,
    pub source: String,
    pub source_map: SourceBundleMap,
}

#[derive(Debug, Clone, Default)]
pub struct SourceBundleMap {
    entries: Vec<SourceLineMapEntry>,
}

#[derive(Debug, Clone)]
struct SourceLineMapEntry {
    assembled_start_line: usize,
    assembled_end_line: usize,
    source_path: PathBuf,
    source_start_line: usize,
}

#[derive(Debug, Deserialize)]
struct SourceBundleManifest {
    schema_version: u32,
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    notes: Option<toml::Value>,
    #[serde(default)]
    topology: Option<SourceBundleSection>,
    #[serde(default)]
    constraints: Option<SourceBundleSection>,
    #[serde(default)]
    tasks: Option<SourceBundleSection>,
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct SourceBundleSection {
    #[serde(default)]
    fragments: Vec<String>,
}

impl SourceBundleMap {
    pub fn plain(path: &Path, source: &str) -> Self {
        let mut map = Self::default();
        let line_count = count_source_lines(source);
        if line_count > 0 {
            map.entries.push(SourceLineMapEntry {
                assembled_start_line: 1,
                assembled_end_line: line_count,
                source_path: absolutize_display_path(path),
                source_start_line: 1,
            });
        }
        map
    }

    pub fn remap_location(&self, line: usize, column: usize) -> Option<SourceLocation> {
        let entry = self
            .entries
            .iter()
            .find(|entry| line >= entry.assembled_start_line && line <= entry.assembled_end_line)?;
        let source_line = entry.source_start_line + (line - entry.assembled_start_line);
        Some(SourceLocation::new(
            entry.source_path.display().to_string(),
            source_line,
            column,
        ))
    }
}

pub fn is_bundle_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.ends_with(BUNDLE_SUFFIX))
        .unwrap_or(false)
}

pub fn is_supported_plc_source_path(path: &Path) -> bool {
    is_bundle_path(path)
        || path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("plc"))
            .unwrap_or(false)
}

pub fn plc_source_stem(path: &Path) -> String {
    if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
        if let Some(stem) = name.strip_suffix(BUNDLE_SUFFIX) {
            if !stem.is_empty() {
                return stem.to_string();
            }
        }
    }

    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("input")
        .to_string()
}

pub fn load_plc_source(path: &Path) -> Result<LoadedPlcSource, String> {
    if is_bundle_path(path) {
        load_bundle_source(path)
    } else {
        let source = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read PLC file {}: {err}", path.display()))?;
        Ok(LoadedPlcSource {
            requested_path: path.to_path_buf(),
            source_map: SourceBundleMap::plain(path, &source),
            source,
        })
    }
}

pub fn remap_plc_error(error: PlcError, source_map: &SourceBundleMap) -> PlcError {
    let location = error.location().clone();
    match source_map.remap_location(location.line, location.column) {
        Some(mapped) => error.with_location(mapped),
        None => error,
    }
}

fn load_bundle_source(path: &Path) -> Result<LoadedPlcSource, String> {
    let manifest_text = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read PLC bundle {}: {err}", path.display()))?;
    let manifest: SourceBundleManifest = toml::from_str(&manifest_text)
        .map_err(|err| format!("Failed to parse PLC bundle {}: {err}", path.display()))?;

    if manifest.schema_version != 1 {
        return Err(format!(
            "Unsupported PLC bundle schema_version {} in {} (expected 1)",
            manifest.schema_version,
            path.display()
        ));
    }

    let unsupported = manifest
        .extra
        .keys()
        .filter(|key| !matches!(key.as_str(), "notes"))
        .cloned()
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "PLC bundle {} uses unsupported sections [{}]; current compiler supports only [{}]",
            path.display(),
            unsupported.join(", "),
            SUPPORTED_BUNDLE_SECTIONS.join(", ")
        ));
    }

    let _ = manifest.entry.as_deref();
    let _ = manifest.mode.as_deref();
    let _ = manifest.notes.as_ref();

    let bundle_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut assembled_lines = Vec::<String>::new();
    let mut source_map = SourceBundleMap::default();

    for (section_name, section) in [
        ("topology", manifest.topology.unwrap_or_default()),
        ("constraints", manifest.constraints.unwrap_or_default()),
        ("tasks", manifest.tasks.unwrap_or_default()),
    ] {
        assembled_lines.push(format!("[{section_name}]"));
        assembled_lines.push(String::new());
        for fragment in section.fragments {
            let fragment_path = bundle_dir.join(&fragment);
            let fragment_text = fs::read_to_string(&fragment_path).map_err(|err| {
                format!(
                    "Failed to read bundle fragment {} referenced from {}: {err}",
                    fragment_path.display(),
                    path.display()
                )
            })?;

            assembled_lines.push(format!(
                "# bundle fragment: {}",
                fragment_path
                    .strip_prefix(bundle_dir)
                    .unwrap_or(&fragment_path)
                    .display()
            ));
            let fragment_lines = fragment_text
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            if !fragment_lines.is_empty() {
                let assembled_start_line = assembled_lines.len() + 1;
                let assembled_end_line = assembled_start_line + fragment_lines.len() - 1;
                source_map.entries.push(SourceLineMapEntry {
                    assembled_start_line,
                    assembled_end_line,
                    source_path: absolutize_display_path(&fragment_path),
                    source_start_line: 1,
                });
                assembled_lines.extend(fragment_lines);
            }
            assembled_lines.push(String::new());
        }
    }

    let mut source = assembled_lines.join("\n");
    if !source.ends_with('\n') {
        source.push('\n');
    }

    Ok(LoadedPlcSource {
        requested_path: path.to_path_buf(),
        source,
        source_map,
    })
}

fn count_source_lines(source: &str) -> usize {
    if source.is_empty() {
        0
    } else {
        source.lines().count()
    }
}

fn absolutize_display_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock works")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn loads_plain_plc_source_with_identity_map() {
        let base = temp_dir("rust_plc_plain_source");
        let plc_path = base.join("demo.plc");
        fs::write(&plc_path, "[topology]\n\n[constraints]\n\n[tasks]\n").expect("write plc");

        let loaded = load_plc_source(&plc_path).expect("load plain plc");
        let remapped = loaded
            .source_map
            .remap_location(1, 1)
            .expect("line 1 should map back");
        assert!(
            remapped.file.ends_with("demo.plc"),
            "expected file path to point at demo.plc, got {}",
            remapped.file
        );
        assert_eq!(remapped.line, 1);
    }

    #[test]
    fn assembles_bundle_sections_and_maps_back_to_fragment_lines() {
        let base = temp_dir("rust_plc_bundle_source");
        let fragments = base.join("fragments");
        fs::create_dir_all(&fragments).expect("create fragments dir");

        fs::write(
            fragments.join("topology.plcfrag"),
            "device plc_main: plc { purpose: \"demo\", model_ref: openplc_softplc }\n",
        )
        .expect("write topology fragment");
        fs::write(fragments.join("constraints.plcfrag"), "").expect("write constraints fragment");
        fs::write(
            fragments.join("tasks.plcfrag"),
            "task main:\n    step idle:\n",
        )
        .expect("write tasks fragment");
        fs::write(
            base.join("demo.bundle.toml"),
            "schema_version = 1\n[topology]\nfragments = [\"fragments/topology.plcfrag\"]\n[constraints]\nfragments = [\"fragments/constraints.plcfrag\"]\n[tasks]\nfragments = [\"fragments/tasks.plcfrag\"]\n",
        )
        .expect("write bundle");

        let loaded = load_plc_source(&base.join("demo.bundle.toml")).expect("load bundle");
        assert!(loaded.source.contains("[topology]"));
        assert!(loaded.source.contains("[constraints]"));
        assert!(loaded.source.contains("[tasks]"));
        assert!(
            loaded
                .source
                .contains("bundle fragment: fragments/topology.plcfrag")
        );

        let topology_line = loaded
            .source
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("device plc_main: plc"))
            .map(|(index, _)| index + 1)
            .expect("assembled source should contain topology fragment");
        let mapped = loaded
            .source_map
            .remap_location(topology_line, 1)
            .expect("topology line should remap");
        assert!(
            mapped.file.ends_with("topology.plcfrag"),
            "expected fragment path, got {}",
            mapped.file
        );
        assert_eq!(mapped.line, 1);
    }

    #[test]
    fn rejects_unsupported_bundle_sections() {
        let base = temp_dir("rust_plc_bundle_unsupported");
        fs::write(
            base.join("demo.bundle.toml"),
            "schema_version = 1\n[io_alias]\nfragments = [\"io_alias.plcfrag\"]\n",
        )
        .expect("write bundle");

        let err = load_plc_source(&base.join("demo.bundle.toml")).expect_err("bundle should fail");
        assert!(err.contains("unsupported sections [io_alias]"));
    }
}
