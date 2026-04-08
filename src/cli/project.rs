use crate::cli_support::common::DispatchResult;
use crate::cli_support::help::command_usage;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let result = match command {
        "new" => run_new_subcommand(program, remaining.iter().cloned()),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix: None,
        result,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectLayout {
    SingleFile,
    StructuredFragments,
}

impl ProjectLayout {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "single-file" => Ok(Self::SingleFile),
            "structured-fragments" => Ok(Self::StructuredFragments),
            other => Err(format!(
                "Unknown layout `{other}` (expected `single-file` or `structured-fragments`)"
            )),
        }
    }

    fn cli_name(self) -> &'static str {
        match self {
            Self::SingleFile => "single-file",
            Self::StructuredFragments => "structured-fragments",
        }
    }

    fn entry_plc_path(self) -> &'static str {
        match self {
            Self::SingleFile => "plc/main.plc",
            Self::StructuredFragments => "plc/main.target_semantics.bundle.toml",
        }
    }

    fn layout_summary(self) -> &'static str {
        match self {
            Self::SingleFile => "single-file PLC source",
            Self::StructuredFragments => "bundle + semantic fragments",
        }
    }
}

fn write_scaffold_file(path: &Path, content: &str, force: bool) -> Result<(), String> {
    if path.exists() && !force {
        return Err(format!(
            "Refusing to overwrite existing file {} (use --force to allow overwrite)",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create directory {}: {err}", parent.display()))?;
        }
    }
    fs::write(path, content).map_err(|err| format!("Failed to write {}: {err}", path.display()))
}

fn prettify_project_name(raw: &str) -> String {
    let parts: Vec<String> = raw
        .split(|c: char| c == '_' || c == '-' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            let mut out = String::new();
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
            out
        })
        .collect();
    if parts.is_empty() {
        "RustPLC Project".to_string()
    } else {
        parts.join(" ")
    }
}

fn run_new_subcommand(program: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let usage = command_usage(program, "new");
    let Some(project_dir) = args.next() else {
        return Err(usage);
    };
    let mut force = false;
    let mut layout = ProjectLayout::SingleFile;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--force" => force = true,
            "--layout" => {
                let value = args.next().ok_or_else(|| {
                    "Missing value for --layout <single-file|structured-fragments>".to_string()
                })?;
                layout = ProjectLayout::parse(&value)?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for new: {other}")),
        }
    }

    let root = PathBuf::from(project_dir);
    if root.exists() {
        if !root.is_dir() {
            return Err(format!(
                "Target path exists but is not a directory: {}",
                root.display()
            ));
        }
        if !force {
            let mut entries = fs::read_dir(&root)
                .map_err(|err| format!("Failed to inspect {}: {err}", root.display()))?;
            if entries.next().is_some() {
                return Err(format!(
                    "Target directory {} is not empty (use --force to overwrite known files)",
                    root.display()
                ));
            }
        }
    } else {
        fs::create_dir_all(&root)
            .map_err(|err| format!("Failed to create {}: {err}", root.display()))?;
    }

    let project_slug = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("rustplc_project")
        .to_string();
    let project_title = prettify_project_name(&project_slug);

    for (relative_path, content) in scaffold_files(&project_slug, &project_title, layout) {
        write_scaffold_file(&root.join(relative_path), &content, force)?;
    }

    eprintln!(
        "new: scaffold created at {} (layout={})",
        root.display(),
        layout.cli_name()
    );
    Ok(())
}

fn scaffold_files(
    project_slug: &str,
    project_title: &str,
    layout: ProjectLayout,
) -> Vec<(String, String)> {
    let entry_plc = layout.entry_plc_path();
    let mut files = vec![
        (
            "README.md".to_string(),
            build_readme(project_slug, project_title, layout),
        ),
        (
            ".gitignore".to_string(),
            "/out/**\n!/out/\n!/out/**/\n!/out/**/.gitkeep\n".to_string(),
        ),
        (
            "rustplc.project.toml".to_string(),
            build_manifest(project_slug, project_title, entry_plc),
        ),
        (
            "plc/main.system.md".to_string(),
            build_system(project_title, project_slug, layout),
        ),
        (
            "scenarios/nominal/normal.yaml".to_string(),
            "tick_ms: 10\nduration_ms: 300\ninputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        0: true\n  - at_ms: 50\n    set:\n      digital_inputs:\n        0: false\nforces: []\n".to_string(),
        ),
        ("scenarios/faults/.gitkeep".to_string(), String::new()),
        ("scenarios/generated/.gitkeep".to_string(), String::new()),
        (
            "config/io_map.toml".to_string(),
            "schema_version = 1\n\n[digital_inputs]\ndi0 = { gpio = 2, pull = \"up\" }\n\n[digital_outputs]\ndo0 = { gpio = 10, active_low = false }\n\n[safe_state]\nmode = \"all_zero\"\non_exit_timeout_ms = 0\n".to_string(),
        ),
        (
            "config/retain.toml".to_string(),
            "schema_version = 1\n\n[retain]\nenabled = false\npath = \"out/sim/retain_state.json\"\n".to_string(),
        ),
        (
            "config/workpiece.toml".to_string(),
            "schema_version = 1\n\n[workpiece]\nrequired = true\n".to_string(),
        ),
        (
            "docs/project-layout.md".to_string(),
            build_project_layout_doc(project_slug, project_title, layout),
        ),
        ("out/ir/.gitkeep".to_string(), String::new()),
        ("out/sim/.gitkeep".to_string(), String::new()),
        ("out/gate/.gitkeep".to_string(), String::new()),
        ("out/codegen/.gitkeep".to_string(), String::new()),
        ("out/rp2040/.gitkeep".to_string(), String::new()),
        ("out/release/.gitkeep".to_string(), String::new()),
        (
            ".github/workflows/no_board_gate.yml".to_string(),
            build_workflow(entry_plc),
        ),
        (
            ".vscode/tasks.json".to_string(),
            build_vscode_tasks(entry_plc),
        ),
        (
            ".vscode/settings.json".to_string(),
            "{\n  \"files.associations\": {\n    \"*.plc\": \"ini\",\n    \"*.plcfrag\": \"ini\"\n  },\n  \"editor.tabSize\": 4,\n  \"editor.insertSpaces\": true,\n  \"editor.detectIndentation\": false\n}\n".to_string(),
        ),
        (
            ".vscode/extensions.json".to_string(),
            "{\n  \"recommendations\": [\n    \"rust-lang.rust-analyzer\",\n    \"redhat.vscode-yaml\",\n    \"tamasfe.even-better-toml\"\n  ]\n}\n".to_string(),
        ),
        (
            ".vscode/plc.code-snippets".to_string(),
            build_vscode_snippets(layout),
        ),
        (
            ".vscode/README.md".to_string(),
            "# VS Code Day-1 Support for RustPLC\n\n- `settings.json`: associates `*.plc` and `*.plcfrag` with INI highlighting\n- `plc.code-snippets`: starter snippets\n- `tasks.json`: one-click project commands\n".to_string(),
        ),
    ];

    match layout {
        ProjectLayout::SingleFile => files.push(("plc/main.plc".to_string(), single_file_plc())),
        ProjectLayout::StructuredFragments => {
            files.extend(structured_fragment_files(project_title));
        }
    }

    files
}

fn build_readme(project_slug: &str, project_title: &str, layout: ProjectLayout) -> String {
    let source_layout = match layout {
        ProjectLayout::SingleFile => {
            "- `plc/main.plc`: executable RustPLC DSL\n- `config/workpiece.toml`: project workpiece policy\n- `scenarios/nominal/normal.yaml`: nominal regression scenario"
        }
        ProjectLayout::StructuredFragments => {
            "- `plc/main.target_semantics.bundle.toml`: DSL source entry\n- `plc/target_semantics_fragments/`: semantic fragment tree\n- `plc/target_semantics_fragments/io|manual|operator_interface|optimization|step/`: authored sidecar semantics kept outside the default compileable bundle when needed\n- `config/workpiece.toml`: project workpiece policy\n- `scenarios/nominal/normal.yaml`: nominal regression scenario"
        }
    };

    format!(
        "# {project_title}\n\n## Project Identity\n\n- Project slug: `{project_slug}`\n- Manifest: `rustplc.project.toml`\n- Source layout: `{}`\n\n## Project Layout\n\n- `plc/main.system.md`: system intent\n{source_layout}\n- `config/io_map.toml`: deployment I/O mapping\n- `config/retain.toml`: retain baseline\n- `out/`: generated artifacts\n\n## Quick Start\n\n```bash\ncargo run --release --bin rust_plc -- project-check {} --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human\n```\n",
        layout.layout_summary(),
        layout.entry_plc_path()
    )
}

fn build_system(project_title: &str, project_slug: &str, layout: ProjectLayout) -> String {
    let source_shape_note = match layout {
        ProjectLayout::SingleFile => {
            "- Preferred Day-1 source shape: single `plc/main.plc` for a minimal starter flow.\n"
        }
        ProjectLayout::StructuredFragments => {
            "- Preferred Day-1 source shape: `plc/main.target_semantics.bundle.toml` plus semantic fragments under `plc/target_semantics_fragments/`.\n"
        }
    };

    format!(
        "# {project_title} System Description\n\n## Project Identity\n- Name: {project_title}\n- Slug: `{project_slug}`\n- Deployment target: demo bench\n{source_shape_note}\n## Process Intent\n1. Wait for the start command.\n2. Energize the run output.\n3. Hold for 20 ms.\n4. De-energize the run output and finish.\n\n## Fault Strategy\n- If the start signal does not arrive within 100 ms, jump to `fault` and de-energize the run output.\n"
    )
}

fn build_manifest(project_slug: &str, project_title: &str, entry_plc: &str) -> String {
    format!(
        "schema_version = 1\n\n[project]\nname = \"{project_title}\"\nslug = \"{project_slug}\"\n\n[entry]\nsystem = \"plc/main.system.md\"\nplc = \"{entry_plc}\"\nscenario = \"scenarios/nominal/normal.yaml\"\nio_map = \"config/io_map.toml\"\nretain = \"config/retain.toml\"\nworkpiece = \"config/workpiece.toml\"\n\n[out]\nir = \"out/ir\"\nsim = \"out/sim\"\ngate = \"out/gate\"\ncodegen = \"out/codegen\"\nrp2040 = \"out/rp2040\"\nrelease = \"out/release\"\n"
    )
}

fn build_project_layout_doc(
    project_slug: &str,
    project_title: &str,
    layout: ProjectLayout,
) -> String {
    let source_lines = match layout {
        ProjectLayout::SingleFile => {
            "- `plc/main.plc`: executable RustPLC DSL entry\n- `config/workpiece.toml`: project workpiece policy\n- `scenarios/nominal/normal.yaml`: nominal scenario"
        }
        ProjectLayout::StructuredFragments => {
            "- `plc/main.target_semantics.bundle.toml`: executable DSL entry\n- `plc/target_semantics_fragments/topology/`: controller, devices, relations, resources\n- `plc/target_semantics_fragments/constraints/`: safety and timing rules\n- `plc/target_semantics_fragments/architecture/`: startup and supervision\n- `plc/target_semantics_fragments/auto/`: automatic production tasks\n- `plc/target_semantics_fragments/maintenance/`: maintenance tasks and self-check sidecars\n- `plc/target_semantics_fragments/manual/`: manual-mode sidecars\n- `plc/target_semantics_fragments/operator_interface/`: operator interface sidecars\n- `plc/target_semantics_fragments/io/`: semantic I/O alias sidecars\n- `plc/target_semantics_fragments/optimization/`: optimization policy sidecars\n- `plc/target_semantics_fragments/step/`: step-mode sidecars\n- `plc/target_semantics_fragments/faults/`: warning and fault tasks\n- `config/workpiece.toml`: project workpiece policy\n- `scenarios/nominal/normal.yaml`: nominal scenario"
        }
    };

    format!(
        "# Project Layout\n\n- `rustplc.project.toml`: project manifest\n- `plc/main.system.md`: system contract\n{source_lines}\n- `config/`: deployment and retain configuration\n- `out/`: generated artifacts\n\nCurrent project: `{project_slug}` / `{project_title}`\nCurrent source layout: `{}`\n",
        layout.layout_summary()
    )
}

fn build_workflow(entry_plc: &str) -> String {
    let template = r#"name: rustplc-no-board-gate

on:
  push:
  pull_request:

jobs:
  no-board-gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Project check
        run: cargo run --release --bin rust_plc -- project-check __ENTRY__ --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output json
"#;
    template.replace("__ENTRY__", entry_plc)
}

fn build_vscode_tasks(entry_plc: &str) -> String {
    let template = r#"{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "RustPLC: project-check",
      "type": "shell",
      "command": "cargo run --release --bin rust_plc -- project-check __ENTRY__ --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human",
      "problemMatcher": []
    },
    {
      "label": "RustPLC: sim-plc",
      "type": "shell",
      "command": "cargo run --release --bin rust_plc -- sim-plc __ENTRY__ --scenario scenarios/nominal/normal.yaml --out out/sim/normal/trace.jsonl",
      "problemMatcher": []
    },
    {
      "label": "RustPLC: no-board-gate",
      "type": "shell",
      "command": "cargo run --release --bin rust_plc -- no-board-gate __ENTRY__ --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human",
      "problemMatcher": []
    }
  ]
}
"#;
    template.replace("__ENTRY__", entry_plc)
}

fn build_vscode_snippets(layout: ProjectLayout) -> String {
    match layout {
        ProjectLayout::SingleFile => "{\n  \"RustPLC: PLC Skeleton\": {\n    \"scope\": \"ini\",\n    \"prefix\": \"plc-skeleton\",\n    \"body\": [\n      \"[topology]\",\n      \"\",\n      \"device plc_main: plc {\",\n      \"    purpose: \\\"Controller with minimal digital I/O mapping\\\",\",\n      \"    model_ref: openplc_softplc\",\n      \"}\",\n      \"\",\n      \"[constraints]\",\n      \"\",\n      \"[tasks]\"\n    ],\n    \"description\": \"Insert a minimal RustPLC file skeleton\"\n  }\n}\n".to_string(),
        ProjectLayout::StructuredFragments => "{\n  \"RustPLC: Bundle Fragment\": {\n    \"scope\": \"ini\",\n    \"prefix\": \"plcfrag\",\n    \"body\": [\n      \"# Semantic fragment placeholder\",\n      \"# Add topology, constraints, or task declarations here\"\n    ],\n    \"description\": \"Insert a semantic fragment placeholder\"\n  }\n}\n".to_string(),
    }
}

fn single_file_plc() -> String {
    "[topology]\n\ndevice plc_main: plc {\n    purpose: \"Controller with minimal digital I/O mapping\",\n    model_ref: openplc_softplc\n}\n\ndevice start_button: sensor { purpose: \"Start request\", subtype: \"push_button\", debounce: 20ms }\ndevice run_lamp: solenoid_valve { purpose: \"Demo run output\", response_time: 20ms }\n\nworkpiece part: workpiece_type {\n    normal_terminal_states: [finished]\n    abnormal_terminal_states: [rejected]\n    ingress_sites: [infeed]\n    normal_egress_sites: [outfeed]\n    abnormal_egress_sites: [reject_bin]\n}\n\nlocation infeed: workpiece_location { capacity: 1 }\nlocation outfeed: workpiece_location { capacity: 1 }\nlocation reject_bin: workpiece_location { capacity: 1 }\nholder part_handler: workpiece_holder { capacity: 1 }\n\nrelation { from: start_button.out, to: plc_main.X0, via: reports_to }\nrelation { from: plc_main.Y0, to: run_lamp.coil, via: driven_by }\n\n[constraints]\n\n[tasks]\n\ntask main:\n    step wait_start:\n        wait: start_button == true\n        timeout: 100ms -> goto fault.reject_unstarted\n\n    step pick:\n        effect: acquire holder part_handler from infeed\n\n    step run:\n        action: set run_lamp.coil on\n        delay: 20ms\n\n    step place:\n        effect: transfer from part_handler to outfeed\n\n    step stop:\n        effect: finish workpiece at outfeed as finished\n        action: set run_lamp.coil off\n\n    on_complete: goto done\n\ntask fault:\n    step reject_unstarted:\n        action: set run_lamp.coil off\n        effect: transfer from infeed to reject_bin\n        effect: finish workpiece at reject_bin as rejected\n    on_complete: goto done\n\ntask done:\n    step halt:\n".to_string()
}

fn structured_fragment_files(project_title: &str) -> Vec<(String, String)> {
    vec![
        (
            "plc/main.target_semantics.bundle.toml".to_string(),
            format!(
                "schema_version = 1\nmode = \"assembly-sketch\"\n\n[notes]\npurpose = \"Structured semantic scaffold for the {project_title} project\"\n\n[topology]\nfragments = [\n  \"target_semantics_fragments/topology/variables.plcfrag\",\n  \"target_semantics_fragments/topology/controller.plcfrag\",\n  \"target_semantics_fragments/topology/interface_devices.plcfrag\",\n  \"target_semantics_fragments/topology/process_devices.plcfrag\",\n  \"target_semantics_fragments/topology/workpieces.plcfrag\",\n  \"target_semantics_fragments/topology/relations.plcfrag\",\n  \"target_semantics_fragments/topology/resources.plcfrag\",\n]\n\n[constraints]\nfragments = [\n  \"target_semantics_fragments/constraints/claims_and_rules.plcfrag\",\n]\n\n[tasks]\nfragments = [\n  \"target_semantics_fragments/architecture/startup_and_supervision.plcfrag\",\n  \"target_semantics_fragments/auto/main_cycle.plcfrag\",\n  \"target_semantics_fragments/faults/common_faults.plcfrag\",\n]\n"
            ),
        ),
        (
            "plc/target_semantics_fragments/topology/variables.plcfrag".to_string(),
            "# Reserved for project-level variables.\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/topology/controller.plcfrag".to_string(),
            format!(
                "device plc_main: plc {{\n    purpose: \"{project_title} controller\"\n    model_ref: openplc_softplc\n}}\n"
            ),
        ),
        (
            "plc/target_semantics_fragments/topology/interface_devices.plcfrag".to_string(),
            "device start_button: sensor { purpose: \"Start request\", subtype: \"push_button\", debounce: 20ms }\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/topology/process_devices.plcfrag".to_string(),
            "device run_lamp: solenoid_valve { purpose: \"Demo run output\", response_time: 20ms }\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/topology/workpieces.plcfrag".to_string(),
            "# Default project workpiece contract.\nworkpiece part: workpiece_type {\n    normal_terminal_states: [finished]\n    abnormal_terminal_states: [rejected]\n    ingress_sites: [infeed]\n    normal_egress_sites: [outfeed]\n    abnormal_egress_sites: [reject_bin]\n}\n\nlocation infeed: workpiece_location { capacity: 1 }\nlocation outfeed: workpiece_location { capacity: 1 }\nlocation reject_bin: workpiece_location { capacity: 1 }\nholder part_handler: workpiece_holder { capacity: 1 }\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/topology/relations.plcfrag".to_string(),
            "relation { from: start_button.out, to: plc_main.X0, via: reports_to }\nrelation { from: plc_main.Y0, to: run_lamp.coil, via: driven_by }\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/topology/resources.plcfrag".to_string(),
            "# Reserved for semantic resources and claims.\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/constraints/claims_and_rules.plcfrag".to_string(),
            "# Reserved for safety, timing, and resource rules.\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/architecture/startup_and_supervision.plcfrag".to_string(),
            "task startup_initializer:\n    step drive_safe_output:\n        action: set run_lamp.coil off\n\n    on_complete: goto supervisor.wait_start\n\ntask supervisor:\n    step wait_start:\n        wait: start_button == true\n        timeout: 100ms -> goto fault.reject_unstarted\n\n    on_complete: goto auto_cycle.pick\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/auto/main_cycle.plcfrag".to_string(),
            "task auto_cycle:\n    step pick:\n        effect: acquire holder part_handler from infeed\n\n    step run:\n        action: set run_lamp.coil on\n        delay: 20ms\n\n    step place:\n        effect: transfer from part_handler to outfeed\n\n    step stop:\n        effect: finish workpiece at outfeed as finished\n        action: set run_lamp.coil off\n\n    on_complete: goto done.halt\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/maintenance/service.plcfrag".to_string(),
            "# Reserved for maintenance-mode tasks.\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/maintenance/self_check.plcfrag".to_string(),
            "# Reserved for dedicated mechanism self-check tasks or a separate self_check bundle.\n"
                .to_string(),
        ),
        (
            "plc/target_semantics_fragments/manual/manual_actions.plcfrag".to_string(),
            "# Reserved for manual-mode tasks.\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/operator_interface/commands_indicators_alarms.plcfrag"
                .to_string(),
            "# Reserved for operator interface tasks and command routing.\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/io/aliases.plcfrag".to_string(),
            "# Reserved for semantic I/O aliases that sit above controller point ids.\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/optimization/candidate_evaluation.plcfrag"
                .to_string(),
            "# Reserved for optimization candidate policy and ranking sidecars.\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/step/step_cycles.plcfrag".to_string(),
            "# Reserved for step-mode execution sidecars that reuse process windows.\n".to_string(),
        ),
        (
            "plc/target_semantics_fragments/faults/common_faults.plcfrag".to_string(),
            "task fault:\n    step reject_unstarted:\n        action: set run_lamp.coil off\n        effect: transfer from infeed to reject_bin\n        effect: finish workpiece at reject_bin as rejected\n\n    on_complete: goto done.halt\n\ntask done:\n    step halt:\n".to_string(),
        ),
    ]
}
