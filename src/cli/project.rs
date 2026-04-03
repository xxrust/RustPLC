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
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--force" => force = true,
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

    let readme = format!(
        "# {project_title}\n\n## Project Identity\n\n- Project slug: `{project_slug}`\n- Manifest: `rustplc.project.toml`\n\n## Project Layout\n\n- `plc/main.system.md`: system intent\n- `plc/main.plc`: executable RustPLC DSL\n- `scenarios/nominal/normal.yaml`: nominal regression scenario\n- `config/io_map.toml`: deployment I/O mapping\n- `config/retain.toml`: retain baseline\n- `out/`: generated artifacts\n\n## Quick Start\n\n```bash\ncargo run --release --bin rust_plc -- project-check plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human\n```\n"
    );
    let gitignore = "/out/**\n!/out/\n!/out/**/\n!/out/**/.gitkeep\n";
    let system = format!(
        "# {project_title} System Description\n\n## Project Identity\n- Name: {project_title}\n- Slug: `{project_slug}`\n- Deployment target: demo bench\n\n## Process Intent\n1. Wait for `X0`.\n2. Turn `Y0` on.\n3. Hold for 20 ms.\n4. Turn `Y0` off and finish.\n\n## Fault Strategy\n- If the start signal does not arrive within 100 ms, jump to `fault` and de-energize `Y0`.\n"
    );
    let plc = "[topology]\n\ndevice plc_main: plc {\n    purpose: \"Controller with minimal digital I/O mapping\",\n    model_ref: openplc_softplc\n}\n\n[constraints]\n\n[tasks]\n\ntask main:\n    step wait_start:\n        wait: X0 == true\n        timeout: 100ms -> goto fault\n\n    step run:\n        action: set Y0 on\n        delay: 20ms\n\n    step stop:\n        action: set Y0 off\n\n    on_complete: goto done\n\ntask fault:\n    step safe_stop:\n        action: set Y0 off\n    on_complete: goto done\n\ntask done:\n    step halt:\n";
    let scenario = "tick_ms: 10\nduration_ms: 300\ninputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        0: true\n  - at_ms: 50\n    set:\n      digital_inputs:\n        0: false\nforces: []\n";
    let io_map = "schema_version = 1\n\n[digital_inputs]\ndi0 = { gpio = 2, pull = \"up\" }\n\n[digital_outputs]\ndo0 = { gpio = 10, active_low = false }\n\n[safe_state]\nmode = \"all_zero\"\non_exit_timeout_ms = 0\n";
    let retain =
        "schema_version = 1\n\n[retain]\nenabled = false\npath = \"out/sim/retain_state.json\"\n";
    let manifest = format!(
        "schema_version = 1\n\n[project]\nname = \"{project_title}\"\nslug = \"{project_slug}\"\n\n[entry]\nsystem = \"plc/main.system.md\"\nplc = \"plc/main.plc\"\nscenario = \"scenarios/nominal/normal.yaml\"\nio_map = \"config/io_map.toml\"\nretain = \"config/retain.toml\"\n\n[out]\nir = \"out/ir\"\nsim = \"out/sim\"\ngate = \"out/gate\"\ncodegen = \"out/codegen\"\nrp2040 = \"out/rp2040\"\nrelease = \"out/release\"\n"
    );
    let project_layout = format!(
        "# Project Layout\n\n- `rustplc.project.toml`: project manifest\n- `plc/`: system and PLC sources\n- `scenarios/`: scenario inputs\n- `config/`: deployment and retain configuration\n- `out/`: generated artifacts\n\nCurrent project: `{project_slug}` / `{project_title}`\n"
    );
    let workflow = "name: rustplc-no-board-gate\n\non:\n  push:\n  pull_request:\n\njobs:\n  no-board-gate:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: dtolnay/rust-toolchain@stable\n      - name: Project check\n        run: cargo run --release --bin rust_plc -- project-check plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output json\n";
    let vscode_tasks = "{\n  \"version\": \"2.0.0\",\n  \"tasks\": [\n    {\n      \"label\": \"RustPLC: project-check\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- project-check plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human\",\n      \"problemMatcher\": []\n    },\n    {\n      \"label\": \"RustPLC: sim-plc\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- sim-plc plc/main.plc --scenario scenarios/nominal/normal.yaml --out out/sim/normal/trace.jsonl\",\n      \"problemMatcher\": []\n    },\n    {\n      \"label\": \"RustPLC: no-board-gate\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human\",\n      \"problemMatcher\": []\n    }\n  ]\n}\n";
    let vscode_settings = "{\n  \"files.associations\": {\n    \"*.plc\": \"ini\"\n  },\n  \"editor.tabSize\": 4,\n  \"editor.insertSpaces\": true,\n  \"editor.detectIndentation\": false\n}\n";
    let vscode_extensions = "{\n  \"recommendations\": [\n    \"rust-lang.rust-analyzer\",\n    \"redhat.vscode-yaml\",\n    \"tamasfe.even-better-toml\"\n  ]\n}\n";
    let vscode_snippets = "{\n  \"RustPLC: PLC Skeleton\": {\n    \"scope\": \"ini\",\n    \"prefix\": \"plc-skeleton\",\n    \"body\": [\n      \"[topology]\",\n      \"\",\n      \"device plc_main: plc {\",\n      \"    purpose: \\\"Controller with minimal digital I/O mapping\\\",\",\n      \"    model_ref: openplc_softplc\",\n      \"}\"\n    ],\n    \"description\": \"Insert a minimal RustPLC file skeleton\"\n  }\n}\n";
    let vscode_readme = "# VS Code Day-1 Support for RustPLC\n\n- `settings.json`: associates `*.plc` with INI highlighting\n- `plc.code-snippets`: starter snippets\n- `tasks.json`: one-click project commands\n";

    write_scaffold_file(&root.join("README.md"), &readme, force)?;
    write_scaffold_file(&root.join(".gitignore"), gitignore, force)?;
    write_scaffold_file(&root.join("rustplc.project.toml"), &manifest, force)?;
    write_scaffold_file(&root.join("plc/main.system.md"), &system, force)?;
    write_scaffold_file(&root.join("plc/main.plc"), plc, force)?;
    write_scaffold_file(&root.join("scenarios/nominal/normal.yaml"), scenario, force)?;
    write_scaffold_file(&root.join("scenarios/faults/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("scenarios/generated/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("config/io_map.toml"), io_map, force)?;
    write_scaffold_file(&root.join("config/retain.toml"), retain, force)?;
    write_scaffold_file(&root.join("docs/project-layout.md"), &project_layout, force)?;
    write_scaffold_file(&root.join("out/ir/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/sim/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/gate/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/codegen/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/rp2040/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/release/.gitkeep"), "", force)?;
    write_scaffold_file(
        &root.join(".github/workflows/no_board_gate.yml"),
        workflow,
        force,
    )?;
    write_scaffold_file(&root.join(".vscode/tasks.json"), vscode_tasks, force)?;
    write_scaffold_file(&root.join(".vscode/settings.json"), vscode_settings, force)?;
    write_scaffold_file(
        &root.join(".vscode/extensions.json"),
        vscode_extensions,
        force,
    )?;
    write_scaffold_file(
        &root.join(".vscode/plc.code-snippets"),
        vscode_snippets,
        force,
    )?;
    write_scaffold_file(&root.join(".vscode/README.md"), vscode_readme, force)?;

    eprintln!("new: scaffold created at {}", root.display());
    Ok(())
}
