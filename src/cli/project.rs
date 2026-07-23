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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeliveryLayer {
    Module,
    Station,
    Line,
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
            Self::StructuredFragments => "rustplc.bundle.toml",
        }
    }

    fn layout_summary(self) -> &'static str {
        match self {
            Self::SingleFile => "single-file PLC source",
            Self::StructuredFragments => "phased bundle (v2)",
        }
    }
}

impl DeliveryLayer {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "module" => Ok(Self::Module),
            "station" => Ok(Self::Station),
            "line" => Ok(Self::Line),
            other => Err(format!(
                "Unknown delivery layer `{other}` (expected `module`, `station`, or `line`)"
            )),
        }
    }

    fn cli_name(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Station => "station",
            Self::Line => "line",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Module => "reusable module",
            Self::Station => "independent station",
            Self::Line => "integrated line",
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
    let mut delivery_layer = DeliveryLayer::Station;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--force" => force = true,
            "--layout" => {
                let value = args.next().ok_or_else(|| {
                    "Missing value for --layout <single-file|structured-fragments>".to_string()
                })?;
                layout = ProjectLayout::parse(&value)?;
            }
            "--delivery-layer" => {
                let value = args.next().ok_or_else(|| {
                    "Missing value for --delivery-layer <module|station|line>".to_string()
                })?;
                delivery_layer = DeliveryLayer::parse(&value)?;
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

    for (relative_path, content) in
        scaffold_files(&project_slug, &project_title, layout, delivery_layer)
    {
        write_scaffold_file(&root.join(relative_path), &content, force)?;
    }

    eprintln!(
        "new: scaffold created at {} (layout={}, delivery_layer={})",
        root.display(),
        layout.cli_name(),
        delivery_layer.cli_name()
    );
    Ok(())
}

fn scaffold_files(
    project_slug: &str,
    project_title: &str,
    layout: ProjectLayout,
    delivery_layer: DeliveryLayer,
) -> Vec<(String, String)> {
    let entry_plc = layout.entry_plc_path();
    let entry_scenario = "scenarios/nominal/normal.yaml";
    let mut files = vec![
        (
            "README.md".to_string(),
            build_readme(
                project_slug,
                project_title,
                layout,
                delivery_layer,
                entry_plc,
                entry_scenario,
            ),
        ),
        (
            ".gitignore".to_string(),
            "/out/**\n!/out/\n!/out/**/\n!/out/**/.gitkeep\n".to_string(),
        ),
        (
            "rustplc.project.toml".to_string(),
            build_manifest(
                project_slug,
                project_title,
                delivery_layer,
                entry_plc,
                entry_scenario,
            ),
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
            "config/state_proof.toml".to_string(),
            scaffold_state_proof_config(),
        ),
        ("out/ir/.gitkeep".to_string(), String::new()),
        ("out/sim/.gitkeep".to_string(), String::new()),
        ("out/gate/.gitkeep".to_string(), String::new()),
        ("out/codegen/.gitkeep".to_string(), String::new()),
        ("out/rp2040/.gitkeep".to_string(), String::new()),
        ("out/release/.gitkeep".to_string(), String::new()),
        (
            ".github/workflows/no_board_gate.yml".to_string(),
            build_workflow(entry_plc, entry_scenario),
        ),
        (
            ".vscode/tasks.json".to_string(),
            build_vscode_tasks(entry_plc, entry_scenario),
        ),
        (
            ".vscode/settings.json".to_string(),
            "{\n  \"files.associations\": {\n    \"*.plc\": \"ini\",\n    \"*.plcfrag\": \"ini\"\n  },\n  \"editor.tabSize\": 4,\n  \"editor.insertSpaces\": true,\n  \"editor.detectIndentation\": false\n}\n".to_string(),
        ),
        (
            ".vscode/extensions.json".to_string(),
            "{\n  \"recommendations\": [\n    \"rust-lang.rust-analyzer\",\n    \"redhat.vscode-yaml\",\n    \"tamasfe.even-better-toml\"\n  ]\n}\n".to_string(),
        ),
        (".vscode/README.md".to_string(), build_vscode_readme()),
        (
            ".vscode/plc.code-snippets".to_string(),
            build_vscode_snippets(layout),
        ),
        (
            "plc/main.system.md".to_string(),
            build_system_doc(project_title, project_slug, delivery_layer),
        ),
        (
            "docs/project-layout.md".to_string(),
            build_project_layout_doc(project_title, project_slug, entry_plc, entry_scenario),
        ),
    ];

    match layout {
        ProjectLayout::SingleFile => {
            files.push(("plc/main.plc".to_string(), single_file_plc()));
        }
        ProjectLayout::StructuredFragments => {
            files.extend(phased_scaffold_files(project_title, delivery_layer));
        }
    }

    files
}

fn scaffold_process_operation_model() -> String {
    r#"schema_version = 1
policy = "opportunistic_admission"
diagnostics = []

# Source-side scheduling intent. Keep this file ahead of task/step flow:
# plc/main.system.md -> process_model/process_operation_model.toml -> 02_process/ -> process-model-check.
# `operation-model` is only a migration or audit helper for an existing task/step source.

[[operation_classes]]
key = "move:Acquire:infeed->part_handler"
operation_ids = ["auto_cycle.pick.op1"]
source_patterns = ["infeed"]
destination_patterns = ["part_handler"]
effect_kinds = ["Acquire"]

[[operation_classes]]
key = "move:Transfer:part_handler->outfeed"
operation_ids = ["auto_cycle.place.op2"]
source_patterns = ["part_handler"]
destination_patterns = ["outfeed"]
effect_kinds = ["Transfer"]

[[operation_classes]]
key = "finish:outfeed:finished"
operation_ids = ["auto_cycle.stop.op3"]
source_patterns = ["outfeed"]
effect_kinds = ["Finish"]

[[operation_classes]]
key = "move:Transfer:infeed->reject_bin+finish:reject_bin:rejected"
operation_ids = ["fault.reject_unstarted.op4"]
source_patterns = ["infeed", "reject_bin"]
destination_patterns = ["reject_bin"]
effect_kinds = ["Finish", "Transfer"]

[[operations]]
id = "auto_cycle.pick.op1"
contract_key = 'effects=[WorkpieceMove { effect: Acquire, from: "infeed", to: "part_handler" }];admissions=[dest:part_handler|source:infeed];resources=[]'
operation_class = "move:Acquire:infeed->part_handler"
task_name = "auto_cycle"
step_name = "pick"

[operations.from_state]
task_name = "auto_cycle"
step_name = "pick"

[operations.to_state]
task_name = "auto_cycle"
step_name = "run"

[operations.guard]
kind = "always"

[[operations.admissions]]
kind = "source_available"
endpoint = "infeed"

[[operations.admissions]]
kind = "destination_has_capacity"
endpoint = "part_handler"

[[operations.effects]]
kind = "workpiece_move"
effect = "acquire"
from = "infeed"
to = "part_handler"

[[operations]]
id = "auto_cycle.place.op2"
contract_key = 'effects=[WorkpieceMove { effect: Transfer, from: "part_handler", to: "outfeed" }];admissions=[dest:outfeed|source:part_handler];resources=[]'
operation_class = "move:Transfer:part_handler->outfeed"
task_name = "auto_cycle"
step_name = "place"

[operations.from_state]
task_name = "auto_cycle"
step_name = "place"

[operations.to_state]
task_name = "auto_cycle"
step_name = "stop"

[operations.guard]
kind = "always"

[[operations.admissions]]
kind = "source_available"
endpoint = "part_handler"

[[operations.admissions]]
kind = "destination_has_capacity"
endpoint = "outfeed"

[[operations.effects]]
kind = "workpiece_move"
effect = "transfer"
from = "part_handler"
to = "outfeed"

[[operations]]
id = "auto_cycle.stop.op3"
contract_key = 'effects=[WorkpieceFinish { at: "outfeed", terminal_state: "finished" }];admissions=[source:outfeed];resources=[]'
operation_class = "finish:outfeed:finished"
task_name = "auto_cycle"
step_name = "stop"

[operations.from_state]
task_name = "auto_cycle"
step_name = "stop"

[operations.to_state]
task_name = "done"
step_name = "halt"

[operations.guard]
kind = "always"

[[operations.admissions]]
kind = "source_available"
endpoint = "outfeed"

[[operations.effects]]
kind = "workpiece_finish"
at = "outfeed"
terminal_state = "finished"

[[operations]]
id = "fault.reject_unstarted.op4"
contract_key = 'effects=[WorkpieceFinish { at: "reject_bin", terminal_state: "rejected" }|WorkpieceMove { effect: Transfer, from: "infeed", to: "reject_bin" }];admissions=[dest:reject_bin|source:infeed|source:reject_bin];resources=[]'
operation_class = "move:Transfer:infeed->reject_bin+finish:reject_bin:rejected"
task_name = "fault"
step_name = "reject_unstarted"

[operations.from_state]
task_name = "fault"
step_name = "reject_unstarted"

[operations.to_state]
task_name = "done"
step_name = "halt"

[operations.guard]
kind = "always"

[[operations.admissions]]
kind = "source_available"
endpoint = "infeed"

[[operations.admissions]]
kind = "destination_has_capacity"
endpoint = "reject_bin"

[[operations.admissions]]
kind = "source_available"
endpoint = "reject_bin"

[[operations.effects]]
kind = "workpiece_move"
effect = "transfer"
from = "infeed"
to = "reject_bin"

[[operations.effects]]
kind = "workpiece_finish"
at = "reject_bin"
terminal_state = "rejected"
"#
    .to_string()
}

fn scaffold_state_proof_config() -> String {
    r#"schema_version = 1

[[trusted_initial_state]]
symbol = "outfeed"
reason = "Starter scaffold assumes the finished-part outfeed is emptied before startup."
proof_basis = "commissioning checklist"

[[trusted_initial_state]]
symbol = "part_handler"
reason = "Starter scaffold assumes the transfer handler is empty before startup."
proof_basis = "commissioning checklist"

[[self_check_exempt_devices]]
device = "run_lamp"
reason = "Starter scaffold output lamp has no modeled feedback contact."
proof_basis = "commissioning checklist verifies lamp wiring during panel test"
"#
    .to_string()
}

// ---------------------------------------------------------------------------
// Single-file layout
// ---------------------------------------------------------------------------

fn single_file_plc() -> String {
    "[topology]\n\ndevice plc_main: plc {\n    purpose: \"Controller with minimal digital I/O mapping\",\n    model_ref: openplc_softplc\n}\n\ncontroller_io plc_main {\n    input start_cycle_cmd: X0 { purpose: \"Start request input\" }\n    output run_lamp_cmd: Y0 { purpose: \"Run lamp command\", safe_state: off }\n}\n\ndevice start_button: sensor { purpose: \"Start request\", subtype: \"push_button\", debounce: 20ms }\ndevice run_lamp: solenoid_valve { purpose: \"Demo run output\", response_time: 20ms }\n\nworkpiece part: workpiece_type {\n    normal_terminal_states: [finished]\n    abnormal_terminal_states: [rejected]\n    ingress_sites: [infeed]\n    normal_egress_sites: [outfeed]\n    abnormal_egress_sites: [reject_bin]\n}\n\nlocation infeed: workpiece_location { capacity: 1 }\nlocation outfeed: workpiece_location { capacity: 1 }\nlocation reject_bin: workpiece_location { capacity: 20 }\nholder part_handler: workpiece_holder { capacity: 1 }\n\nrelation { from: start_button.out, to: plc_main.start_cycle_cmd, via: reports_to }\nrelation { from: plc_main.run_lamp_cmd, to: run_lamp.coil, via: driven_by }\n\n[constraints]\n\n[tasks]\n\ntask startup_initializer:\n    step safe_outputs_off:\n        action: set run_lamp off\n\n    on_complete: goto main.wait_start\n\ntask main:\n    step wait_start:\n        wait: start_button == true\n        timeout: 100ms -> goto fault.reject_unstarted\n\n    step pick:\n        effect: acquire holder part_handler from infeed\n\n    step run:\n        action: set run_lamp on\n        delay: 20ms\n\n    step place:\n        effect: transfer from part_handler to outfeed\n\n    step stop:\n        effect: finish workpiece at outfeed as finished\n        action: set run_lamp off\n\n    on_complete: goto done\n\ntask fault:\n    step reject_unstarted:\n        action: set run_lamp off\n        effect: transfer from infeed to reject_bin\n        effect: finish workpiece at reject_bin as rejected\n    on_complete: goto done\n\ntask done:\n    step halt:\n".to_string()
}

// ---------------------------------------------------------------------------
// Phased (structured-fragments v2) layout
// ---------------------------------------------------------------------------

fn phased_scaffold_files(
    project_title: &str,
    delivery_layer: DeliveryLayer,
) -> Vec<(String, String)> {
    vec![
        // -- bundle entry --
        ("rustplc.bundle.toml".to_string(), build_bundle_v2()),
        (
            "process_model/process_operation_model.toml".to_string(),
            scaffold_process_operation_model(),
        ),
        // -- 00_topology: devices, connections, workpieces --
        (
            "00_topology/controller.plc".to_string(),
            format!(
                "device plc_main: plc {{\n    purpose: \"{project_title} controller\"\n    model_ref: openplc_softplc\n}}\n\ncontroller_io plc_main {{\n    input start_cycle_cmd: X0 {{ purpose: \"Start request input\" }}\n    output run_lamp_cmd: Y0 {{ purpose: \"Run lamp command\", safe_state: off }}\n}}\n"
            ),
        ),
        (
            "00_topology/devices.plc".to_string(),
            "device start_button: sensor { purpose: \"Start request\", subtype: \"push_button\", debounce: 20ms }\ndevice run_lamp: solenoid_valve { purpose: \"Demo run output\", response_time: 20ms }\n".to_string(),
        ),
        (
            "00_topology/workpieces.plc".to_string(),
            "workpiece part: workpiece_type {\n    normal_terminal_states: [finished]\n    abnormal_terminal_states: [rejected]\n    ingress_sites: [infeed]\n    normal_egress_sites: [outfeed]\n    abnormal_egress_sites: [reject_bin]\n}\n\nlocation infeed: workpiece_location { capacity: 1 }\nlocation outfeed: workpiece_location { capacity: 1 }\nlocation reject_bin: workpiece_location { capacity: 20 }\nholder part_handler: workpiece_holder { capacity: 1 }\n".to_string(),
        ),
        (
            "00_topology/connections.plc".to_string(),
            "relation { from: start_button.out, to: plc_main.start_cycle_cmd, via: reports_to }\nrelation { from: plc_main.run_lamp_cmd, to: run_lamp.coil, via: driven_by }\n".to_string(),
        ),
        (
            "00_topology/_station_protocol.plc".to_string(),
            concat!(
                "# Station isolation protocol.\n",
                "#\n",
                "# When a project has multiple stations, this file declares:\n",
                "#   1. Device partition - which station owns which devices\n",
                "#   2. Handshake signals - how stations communicate\n",
                "#   3. Transfer points - where workpieces cross station boundaries\n",
                "#\n",
                "# Without these declarations, multi-station files in 02_process/\n",
                "# have NO compiler-enforced isolation. Any task can write any device.\n",
                "#\n",
                "# Example (supported by the compiler):\n",
                "#\n",
                "#   station st01_loading {\n",
                "#       owns: [valve_push, cyl_push, sensor_push_ext, sensor_push_ret]\n",
                "#       tasks: [st01_cycle]\n",
                "#   }\n",
                "#\n",
                "#   station st02_assembly {\n",
                "#       owns: [valve_press, cyl_press, sensor_press_ext, sensor_press_ret]\n",
                "#       tasks: [st02_cycle]\n",
                "#   }\n",
                "#\n",
                "#   handshake st01_to_st02 {\n",
                "#       from: st01_loading, to: st02_assembly\n",
                "#       request: st01_outflow_request\n",
                "#       allow: st02_inflow_allow\n",
                "#       complete: st01_outflow_done\n",
                "#       timeout: 5000ms -> goto fault\n",
                "#   }\n",
                "#\n",
                "#   transfer_point st01_st02_handoff {\n",
                "#       from_station: st01_loading\n",
                "#       to_station: st02_assembly\n",
                "#       site: press_position\n",
                "#       handshake: st01_to_st02\n",
                "#   }\n",
            ).to_string(),
        ),
        // -- 01_init: initialization defaults --
        (
            "01_init/defaults.plc".to_string(),
            "task startup_initializer:\n    step safe_outputs_off:\n        action: set run_lamp off\n\n    on_complete: goto supervisor.wait_start\n".to_string(),
        ),
        // -- 02_process: automatic production cycle --
        (
            "02_process/main_cycle.plc".to_string(),
            "task supervisor:\n    step wait_start:\n        wait: start_button == true\n        timeout: 100ms -> goto fault.reject_unstarted\n\n    on_complete: goto auto_cycle.pick\n\ntask auto_cycle:\n    step pick:\n        effect: acquire holder part_handler from infeed\n\n    step run:\n        action: set run_lamp on\n        delay: 20ms\n\n    step place:\n        effect: transfer from part_handler to outfeed\n\n    step stop:\n        effect: finish workpiece at outfeed as finished\n        action: set run_lamp off\n\n    on_complete: goto done.halt\n".to_string(),
        ),
        // -- 03_constraints: safety and timing rules --
        (
            "03_constraints/_placeholder.plc".to_string(),
            "# Safety, timing, and resource constraint rules.\n# This file is a placeholder; the compiler skips files starting with _.\n# Rename to e.g. safety_rules.plc when adding real constraints.\n".to_string(),
        ),
        // -- 04_faults: fault handling --
        (
            "04_faults/fault_handlers.plc".to_string(),
            "task fault:\n    step reject_unstarted:\n        action: set run_lamp off\n        effect: transfer from infeed to reject_bin\n        effect: finish workpiece at reject_bin as rejected\n\n    on_complete: goto done.halt\n\ntask done:\n    step halt:\n".to_string(),
        ),
        // -- 05_supervision: mode management (placeholder) --
        (
            "05_supervision/_placeholder.plc".to_string(),
            "# Machine mode management (auto/manual/init/alarm).\n# This file is a placeholder; the compiler skips files starting with _.\n".to_string(),
        ),
        // -- 06_manual: manual mode (placeholder) --
        (
            "06_manual/_placeholder.plc".to_string(),
            "# Manual-mode tasks and jog operations.\n# This file is a placeholder; the compiler skips files starting with _.\n".to_string(),
        ),
        // -- 07_hmi: HMI interface (placeholder) --
        (
            "07_hmi/_placeholder.plc".to_string(),
            "# HMI command routing and status mirroring.\n# This file is a placeholder; the compiler skips files starting with _.\n".to_string(),
        ),
        // -- docs --
        (
            "docs/system.md".to_string(),
            build_system_doc(project_title, "", delivery_layer),
        ),
        (
            "docs/architecture.md".to_string(),
            build_architecture_doc(project_title, delivery_layer),
        ),
        (
            "docs/verification.md".to_string(),
            build_verification_doc(project_title),
        ),
    ]
}

fn build_bundle_v2() -> String {
    r#"schema_version = 2

# ============================================================================
# Execution order & agent collaboration
# ============================================================================
#
# Phases execute in STRICT SERIAL order (top to bottom).
# Each phase assumes the state established by all previous phases.
#
#   00_topology     -> declare devices, workpieces, connections, station protocol
#   process_model   -> author admissible workpiece operations before task/step
#   01_init         -> establish safe state (all outputs off, defaults set)
#   02_process      -> automatic production cycle (assumes safe-state entry)
#   03_constraints  -> safety/timing rules (references devices + steps)
#   04_faults       -> fault recovery (returns system to safe state)
#   05_supervision  -> mode arbitration (placeholder)
#   06_manual       -> manual operations (placeholder)
#   07_hmi          -> HMI interface (placeholder)
#
# PARALLEL WINDOW (within 02_process/ and 04_faults/):
#   Multiple station files (st01_*.plc, st02_*.plc) can be written by
#   separate agents simultaneously, BUT ONLY IF the station protocol in
#   00_topology/_station_protocol.plc declares:
#     - Device partition: which station owns which devices
#     - Handshake signals: how stations communicate
#     - Transfer points: where workpieces cross station boundaries
#
#   Without a station protocol, there is NO compiler-enforced isolation.
#   Any task can write any device. Parallel authoring is at your own risk.
#
# ============================================================================

[phases.00_topology]
path = "00_topology/"
section = "topology"
exports = ["devices", "connections", "workpieces"]

[phases.01_init]
path = "01_init/"
section = "tasks"
depends_on = ["00_topology"]
exports = ["startup_initializer"]
# Establishes: all outputs in safe state, device defaults applied.
# Downstream phases assume this state as their entry condition.

[phases.02_process]
path = "02_process/"
section = "tasks"
depends_on = ["01_init"]
exports = ["supervisor", "auto_cycle"]
# Entry condition: safe state established by 01_init.
# Multi-station: add st01_*.plc, st02_*.plc - requires station protocol.

[phases.03_constraints]
path = "03_constraints/"
section = "constraints"
depends_on = ["02_process"]
exports = ["safety_rules", "timing_rules"]
# References device names AND step/task names from 02_process.

[phases.04_faults]
path = "04_faults/"
section = "tasks"
depends_on = ["02_process"]
exports = ["fault", "done"]
# Handles abnormal states created by 02_process.
# Returns system to safe state defined by 01_init.

[phases.05_supervision]
path = "05_supervision/"
section = "tasks"
depends_on = ["02_process"]
enabled = false
# Arbitrates auto/manual/init modes across 02_process tasks.

[phases.06_manual]
path = "06_manual/"
section = "tasks"
depends_on = ["01_init"]
enabled = false
# Manual operations assume safe state, independent of 02_process.

[phases.07_hmi]
path = "07_hmi/"
section = "tasks"
depends_on = ["05_supervision"]
enabled = false
# Mirrors supervision state to HMI.
"#
    .to_string()
}

// ---------------------------------------------------------------------------
// Shared builders
// ---------------------------------------------------------------------------

fn build_readme(
    project_slug: &str,
    project_title: &str,
    layout: ProjectLayout,
    delivery_layer: DeliveryLayer,
    entry_plc: &str,
    entry_scenario: &str,
) -> String {
    let structure = match layout {
        ProjectLayout::SingleFile => {
            "- `plc/main.plc`: all-in-one PLC source\n- `config/`: deployment configuration\n- `scenarios/`: test scenarios"
        }
        ProjectLayout::StructuredFragments => {
            "- `00_topology/`: device declarations, workpieces, connections\n- `process_model/`: authored process operation scheduling intent\n- `01_init/`: initialization and startup tasks\n- `02_process/`: automatic production cycle\n- `03_constraints/`: safety and timing rules\n- `04_faults/`: fault handling tasks\n- `05_supervision/`: mode management (placeholder)\n- `06_manual/`: manual-mode tasks (placeholder)\n- `07_hmi/`: HMI interface (placeholder)\n- `config/`: deployment configuration\n- `scenarios/`: test scenarios\n- `docs/`: project documentation"
        }
    };

    format!(
        "# {project_title}\n\n- Project slug: `{project_slug}`\n- Manifest: `rustplc.project.toml`\n- Layout: `{}`\n- Delivery layer: `{}` ({})\n\n## Structure\n\n{structure}\n\n## Quick Start\n\n```bash\ncargo run --release --bin rust_plc -- project-check {entry_plc} --scenario {entry_scenario} --out-dir out/check --output human\n```\n",
        layout.layout_summary(),
        delivery_layer.cli_name(),
        delivery_layer.label()
    )
}

fn build_manifest(
    project_slug: &str,
    project_title: &str,
    delivery_layer: DeliveryLayer,
    entry_plc: &str,
    entry_scenario: &str,
) -> String {
    format!(
        "schema_version = 1\n\n[project]\nname = \"{project_title}\"\nslug = \"{project_slug}\"\n\n[delivery]\nlayer = \"{}\"\n\n[entry]\nsystem = \"plc/main.system.md\"\nplc = \"{entry_plc}\"\nscenario = \"{entry_scenario}\"\nio_map = \"config/io_map.toml\"\nretain = \"config/retain.toml\"\nworkpiece = \"config/workpiece.toml\"\n\n[out]\nir = \"out/ir\"\nsim = \"out/sim\"\ngate = \"out/gate\"\ncodegen = \"out/codegen\"\nrp2040 = \"out/rp2040\"\nrelease = \"out/release\"\n",
        delivery_layer.cli_name()
    )
}

fn build_system_doc(
    project_title: &str,
    project_slug: &str,
    delivery_layer: DeliveryLayer,
) -> String {
    format!(
        "# {project_title} System Description\n\n## Identity\n- Name: {project_title}\n- Project slug: `{project_slug}`\n- Delivery layer: `{}` ({})\n\n## Process Intent\n1. Wait for the start command.\n2. Energize the run output.\n3. Hold for 20 ms.\n4. De-energize the run output and finish.\n\n## Fault Strategy\n- If the start signal does not arrive within 100 ms, jump to `fault` and de-energize the run output.\n",
        delivery_layer.cli_name(),
        delivery_layer.label()
    )
}

fn build_project_layout_doc(
    project_title: &str,
    project_slug: &str,
    entry_plc: &str,
    entry_scenario: &str,
) -> String {
    format!(
        "# Project Layout\n\nThis scaffold uses the standard RustPLC project layout.\n\n- `rustplc.project.toml`: project manifest and default artifact paths\n- `plc/main.system.md`: human/AI confirmed system intent\n- `process_model/process_operation_model.toml`: authored operation scheduling intent, before task/step\n- `{entry_plc}`: executable RustPLC source entry\n- `{entry_scenario}`: nominal regression scenario\n- `config/`: I/O, retain, workpiece, and state-proof configuration\n- `out/`: rebuildable generated artifacts\n\nCurrent project: `{project_slug}` / `{project_title}`\n\nRecommended commands:\n\n```bash\ncargo run --release --bin rust_plc -- process-model-check \\\n  {entry_plc} --model process_model/process_operation_model.toml --output human\n\ncargo run --release --bin rust_plc -- state-proof-check \\\n  {entry_plc} --config config/state_proof.toml --output human\n\ncargo run --release --bin rust_plc -- scenario-validate \\\n  {entry_plc} --scenario {entry_scenario} --output human\n\ncargo run --release --bin rust_plc -- sim-plc \\\n  {entry_plc} --scenario {entry_scenario} --out out/sim/normal/trace.jsonl\n\ncargo run --release --bin rust_plc -- no-board-gate \\\n  {entry_plc} --scenario {entry_scenario} \\\n  --out-dir out/gate/no_board/normal --output human\n\ncargo run --release --bin rust_plc -- gen-st \\\n  {entry_plc} --out out/codegen/st/main.st\n```\n"
    )
}

fn build_vscode_readme() -> String {
    "# VS Code Day-1 Support for RustPLC\n\n## What this package provides\n\n- `settings.json`: associates `*.plc` with INI highlighting\n- `plc.code-snippets`: starter snippets for PLC skeletons\n- `tasks.json`: one-click project-check, sim, and gate commands\n- `extensions.json`: recommended Rust/YAML/TOML extensions\n\n## Troubleshooting\n\n1. If snippets do not appear, confirm the file is `*.plc` and reload the window.\n2. If tasks fail with `command not found`, run them from the workspace root with `cargo` on PATH.\n3. If YAML/TOML diagnostics are missing, install the recommended extensions.\n".to_string()
}

fn build_architecture_doc(project_title: &str, delivery_layer: DeliveryLayer) -> String {
    format!(
        r#"# {project_title} Architecture

## Delivery Layer
- `{}`

## Execution Order

Phases are strictly serial. Each phase assumes the state established by
all previous phases. The numbered prefix IS the execution order.

```
00_topology      declare devices, workpieces, connections, station protocol
      |
01_init          establish safe state (all outputs off, defaults applied)
      |
02_process       automatic production cycle (entry: safe state)
      |
03_constraints   safety & timing rules (references steps from 02)
      |
04_faults        fault recovery (returns to safe state from 01)
      |
05_supervision   mode arbitration [placeholder]
      |
06_manual        manual operations [placeholder]
      |
07_hmi           HMI mirroring [placeholder]
```

### Why strictly serial?

PLC programs have state-precondition dependencies, not just symbol dependencies:

- 01_init establishes the safe state. 02_process assumes it as entry condition.
- 02_process creates abnormal states. 04_faults must know what those states are.
- 03_constraints references step names from 02_process.

An agent writing 02_process must know what 01_init guarantees.
An agent writing 04_faults must know what 02_process can break.

## Multi-Station Parallel Authoring

The only parallel window is WITHIN a phase: multiple station files inside
02_process/ or 04_faults/ can be written by separate agents simultaneously.

### Prerequisites (station protocol)

Parallel authoring requires a station protocol declared in
`00_topology/_station_protocol.plc`. Without it, there is NO compiler-enforced
isolation - any task can write any device.

The protocol has three parts:

1. Device partition - which station owns which devices.
   Compiler rejects a task that writes a device it does not own.

2. Handshake signals - how stations communicate.
   Compiler verifies timeout handling and deadlock freedom.

3. Transfer points - where workpieces cross station boundaries.
   Compiler verifies capacity constraints and flow continuity.

See `00_topology/_station_protocol.plc` for the DSL draft syntax.

### Example: 3-station assembly line

```
Phase 1 - Architect (serial):
    00_topology/
        controller.plc
        st01_devices.plc
        st02_devices.plc
        st03_devices.plc
        workpieces.plc
        connections.plc
        _station_protocol.plc    <- device partition + handshakes

Phase 2 - Architect (serial):
    01_init/defaults.plc         <- safe state for all stations

Phase 3 - Station agents (PARALLEL, guarded by station protocol):
    02_process/
        st01_loading.plc         <- Agent A (only writes st01 devices)
        st02_assembly.plc        <- Agent B (only writes st02 devices)
        st03_inspection.plc      <- Agent C (only writes st03 devices)

Phase 4 - Safety agent (serial, after all stations done):
    03_constraints/safety.plc

Phase 5 - Fault agents (PARALLEL, same partition as Phase 3):
    04_faults/
        st01_faults.plc          <- Agent A
        st02_faults.plc          <- Agent B
        st03_faults.plc          <- Agent C
```

### What each agent reads

| Agent role       | Must read                                | Writes                 |
|------------------|------------------------------------------|------------------------|
| Architect        | requirements doc                         | 00_topology/, 01_init/ |
| Station agent    | 00_topology exports + station protocol   | 02_process/st_XX.plc   |
| Fault agent      | 00_topology exports + 02_process exports | 04_faults/st_XX.plc    |
| Safety agent     | 00_topology + 02_process exports         | 03_constraints/        |

The `exports` field in `rustplc.bundle.toml` is the interface contract.
Agents read exports and the station protocol, not source files from other phases.
"#,
        delivery_layer.label()
    )
}

fn build_verification_doc(project_title: &str) -> String {
    format!(
        "# {project_title} Verification\n\n## Required Checks\n1. Compile the bundle: `rustplc.bundle.toml`\n2. Check the source-side process model: `process_model/process_operation_model.toml`\n3. Run scenario: `scenarios/nominal/normal.yaml`\n4. Run `sim-plc` and inspect trace\n5. Run `no-board-gate` when scenario is stable\n\n## Commands\n```bash\ncargo run --release --bin rust_plc -- process-model-check rustplc.bundle.toml --model process_model/process_operation_model.toml --output human\ncargo run --release --bin rust_plc -- project-check rustplc.bundle.toml --scenario scenarios/nominal/normal.yaml --out-dir out/check --output human\ncargo run --release --bin rust_plc -- sim-plc rustplc.bundle.toml --scenario scenarios/nominal/normal.yaml --out out/sim/trace.jsonl\n```\n"
    )
}

fn build_workflow(entry_plc: &str, entry_scenario: &str) -> String {
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
        run: cargo run --release --bin rust_plc -- project-check __ENTRY__ --scenario __SCENARIO__ --out-dir out/project_check/normal --output json
"#;
    template
        .replace("__ENTRY__", entry_plc)
        .replace("__SCENARIO__", entry_scenario)
}

fn build_vscode_tasks(entry_plc: &str, entry_scenario: &str) -> String {
    let template = r#"{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "RustPLC: project-check",
      "type": "shell",
      "command": "cargo run --release --bin rust_plc -- project-check __ENTRY__ --scenario __SCENARIO__ --out-dir out/project_check/normal --output human",
      "problemMatcher": []
    },
    {
      "label": "RustPLC: sim-plc",
      "type": "shell",
      "command": "cargo run --release --bin rust_plc -- sim-plc __ENTRY__ --scenario __SCENARIO__ --out out/sim/normal/trace.jsonl",
      "problemMatcher": []
    },
    {
      "label": "RustPLC: no-board-gate",
      "type": "shell",
      "command": "cargo run --release --bin rust_plc -- no-board-gate __ENTRY__ --scenario __SCENARIO__ --out-dir out/gate/no_board/normal --output human",
      "problemMatcher": []
    }
  ]
}
"#;
    template
        .replace("__ENTRY__", entry_plc)
        .replace("__SCENARIO__", entry_scenario)
}

fn build_vscode_snippets(layout: ProjectLayout) -> String {
    match layout {
        ProjectLayout::SingleFile => "{\n  \"RustPLC: PLC Skeleton\": {\n    \"scope\": \"ini\",\n    \"prefix\": \"plc-skeleton\",\n    \"body\": [\n      \"[topology]\",\n      \"\",\n      \"device plc_main: plc {\",\n      \"    purpose: \\\"Controller with minimal digital I/O mapping\\\",\",\n      \"    model_ref: openplc_softplc\",\n      \"}\",\n      \"\",\n      \"[constraints]\",\n      \"\",\n      \"[tasks]\"\n    ],\n    \"description\": \"Insert a minimal RustPLC file skeleton\"\n  }\n}\n".to_string(),
        ProjectLayout::StructuredFragments => "{\n  \"RustPLC: Phase File\": {\n    \"scope\": \"ini\",\n    \"prefix\": \"plc-phase\",\n    \"body\": [\n      \"# Phase: ${1:00_topology}\",\n      \"# Add device, constraint, or task declarations here\"\n    ],\n    \"description\": \"Insert a phase file header\"\n  }\n}\n".to_string(),
    }
}
