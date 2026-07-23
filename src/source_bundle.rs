use crate::error::{PlcError, SourceLocation};
use serde::Deserialize;
use sha2::{Digest, Sha256};
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
    pub dependencies: Vec<LoadedSourceDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSourceDependency {
    pub role: String,
    pub path: PathBuf,
    pub sha256: String,
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
    #[serde(default)]
    phases: Option<BTreeMap<String, BundlePhase>>,
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct SourceBundleSection {
    #[serde(default)]
    fragments: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BundlePhase {
    path: String,
    section: String,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    exports: Vec<String>,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
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
        let dependencies = vec![source_dependency("source", path, source.as_bytes())];
        Ok(LoadedPlcSource {
            requested_path: path.to_path_buf(),
            source_map: SourceBundleMap::plain(path, &source),
            source,
            dependencies,
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
    let manifest_dependency = source_dependency("bundle_manifest", path, manifest_text.as_bytes());
    let manifest: SourceBundleManifest = toml::from_str(&manifest_text)
        .map_err(|err| format!("Failed to parse PLC bundle {}: {err}", path.display()))?;

    match manifest.schema_version {
        1 => load_bundle_v1(path, manifest, manifest_dependency),
        2 => load_bundle_v2(path, manifest, manifest_dependency),
        other => Err(format!(
            "Unsupported PLC bundle schema_version {} in {} (expected 1 or 2)",
            other,
            path.display()
        )),
    }
}

fn load_bundle_v1(
    path: &Path,
    manifest: SourceBundleManifest,
    manifest_dependency: LoadedSourceDependency,
) -> Result<LoadedPlcSource, String> {
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
    let mut dependencies = vec![manifest_dependency];

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
            dependencies.push(source_dependency(
                &format!("bundle_fragment:{section_name}"),
                &fragment_path,
                fragment_text.as_bytes(),
            ));

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
        dependencies,
    })
}

fn load_bundle_v2(
    path: &Path,
    manifest: SourceBundleManifest,
    manifest_dependency: LoadedSourceDependency,
) -> Result<LoadedPlcSource, String> {
    let phases = manifest
        .phases
        .ok_or_else(|| format!("PLC bundle v2 {} requires a [phases] table", path.display()))?;

    let bundle_dir = path.parent().unwrap_or_else(|| Path::new("."));

    let mut export_owners = BTreeMap::<String, String>::new();
    let mut topology_fragments = Vec::new();
    let mut constraint_fragments = Vec::new();
    let mut task_fragments = Vec::new();

    let mut sorted_phases: Vec<_> = phases.into_iter().collect();
    sorted_phases.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (phase_name, phase) in &sorted_phases {
        if !phase.enabled {
            continue;
        }
        let valid_sections = ["topology", "constraints", "tasks"];
        if !valid_sections.contains(&phase.section.as_str()) {
            return Err(format!(
                "Phase `{}` in {} has invalid section `{}` (expected one of: {})",
                phase_name,
                path.display(),
                phase.section,
                valid_sections.join(", ")
            ));
        }
        for dep in &phase.depends_on {
            if !sorted_phases.iter().any(|(name, _)| name == dep) {
                return Err(format!(
                    "Phase `{}` in {} depends on unknown phase `{}`",
                    phase_name,
                    path.display(),
                    dep
                ));
            }
        }
        for export in &phase.exports {
            let export = export.trim();
            if export.is_empty() {
                return Err(format!(
                    "Phase `{}` in {} declares an empty export name",
                    phase_name,
                    path.display()
                ));
            }
            if let Some(previous_phase) =
                export_owners.insert(export.to_string(), phase_name.to_string())
            {
                return Err(format!(
                    "Export `{}` in {} is declared by both phase `{}` and phase `{}`",
                    export,
                    path.display(),
                    previous_phase,
                    phase_name
                ));
            }
        }

        let phase_dir = bundle_dir.join(&phase.path);
        let plc_files = collect_plc_files_sorted(&phase_dir).map_err(|err| {
            format!(
                "Failed to scan phase directory {} for phase `{}` in {}: {err}",
                phase_dir.display(),
                phase_name,
                path.display()
            )
        })?;

        let target = match phase.section.as_str() {
            "topology" => &mut topology_fragments,
            "constraints" => &mut constraint_fragments,
            "tasks" => &mut task_fragments,
            _ => unreachable!(),
        };
        for plc_file in plc_files {
            target.push(plc_file);
        }
    }

    let mut assembled_lines = Vec::<String>::new();
    let mut source_map = SourceBundleMap::default();
    let mut dependencies = vec![manifest_dependency];

    for (section_name, fragments) in [
        ("topology", topology_fragments),
        ("constraints", constraint_fragments),
        ("tasks", task_fragments),
    ] {
        assembled_lines.push(format!("[{section_name}]"));
        assembled_lines.push(String::new());
        for fragment_path in fragments {
            let fragment_text = fs::read_to_string(&fragment_path).map_err(|err| {
                format!(
                    "Failed to read phase file {} referenced from {}: {err}",
                    fragment_path.display(),
                    path.display()
                )
            })?;
            dependencies.push(source_dependency(
                &format!("bundle_fragment:{section_name}"),
                &fragment_path,
                fragment_text.as_bytes(),
            ));

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
        dependencies,
    })
}

fn collect_plc_files_sorted(dir: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('_') {
                continue;
            }
            match path.extension().and_then(|e| e.to_str()) {
                Some("plc") | Some("plcfrag") => files.push(path),
                _ => {}
            }
        }
    }
    files.sort();
    Ok(files)
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

fn source_dependency(role: &str, path: &Path, bytes: &[u8]) -> LoadedSourceDependency {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    LoadedSourceDependency {
        role: role.to_string(),
        path: absolutize_display_path(path),
        sha256: hex::encode(hasher.finalize()),
    }
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

    #[test]
    fn rejects_duplicate_v2_phase_exports() {
        let base = temp_dir("rust_plc_bundle_v2_duplicate_exports");
        fs::write(
            base.join("demo.bundle.toml"),
            "schema_version = 2\n\
             [phases.00_topology]\n\
             path = \"00_topology/\"\n\
             section = \"topology\"\n\
             exports = [\"shared\"]\n\
             [phases.01_init]\n\
             path = \"01_init/\"\n\
             section = \"tasks\"\n\
             exports = [\"shared\"]\n",
        )
        .expect("write bundle");

        let err = load_plc_source(&base.join("demo.bundle.toml")).expect_err("bundle should fail");
        assert!(err.contains("Export `shared`"));
        assert!(err.contains("phase `00_topology`"));
        assert!(err.contains("phase `01_init`"));
    }

    #[test]
    fn rejects_empty_v2_phase_export_names() {
        let base = temp_dir("rust_plc_bundle_v2_empty_export");
        fs::write(
            base.join("demo.bundle.toml"),
            "schema_version = 2\n\
             [phases.00_topology]\n\
             path = \"00_topology/\"\n\
             section = \"topology\"\n\
             exports = [\"\"]\n",
        )
        .expect("write bundle");

        let err = load_plc_source(&base.join("demo.bundle.toml")).expect_err("bundle should fail");
        assert!(err.contains("declares an empty export name"));
    }
}
