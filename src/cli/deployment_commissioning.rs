fn run_flash_rp2040_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "flash-rp2040");
    let mut uf2: Option<PathBuf> = None;
    let mut mount: Option<PathBuf> = None;
    let mut dry_run = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--uf2" => {
                uf2 =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --uf2 <file.uf2>".to_string()
                    })?));
            }
            "--mount" => {
                mount =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --mount <path>".to_string()
                    })?));
            }
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for flash-rp2040: {other}")),
        }
    }

    let uf2 = uf2.ok_or_else(|| usage.clone())?;
    let mount = mount.ok_or_else(|| usage.clone())?;

    if !uf2.exists() {
        return Err(format!("UF2 file does not exist: {uf2:?}"));
    }
    if !mount.exists() {
        return Err(format!("Mount path does not exist: {mount:?}"));
    }
    if !mount.is_dir() {
        return Err(format!("Mount path is not a directory: {mount:?}"));
    }

    let file_name = uf2
        .file_name()
        .ok_or_else(|| format!("Invalid UF2 path (no file name): {uf2:?}"))?;
    let dest = mount.join(file_name);

    if dry_run {
        eprintln!("dry-run: would copy {uf2:?} -> {dest:?}");
        return Ok(());
    }

    fs::copy(&uf2, &dest).map_err(|err| {
        format!("Failed to copy UF2 to mount (src={uf2:?}, dest={dest:?}): {err}")
    })?;
    Ok(())
}

fn run_board_parse_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "board-parse");
    let mut input: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--in" => {
                input =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --in <board.log>".to_string()
                    })?));
            }
            "--out-dir" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out-dir <dir>".to_string()
                    })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for board-parse: {other}")),
        }
    }

    let input = input.ok_or_else(|| usage.clone())?;
    let out_dir = out_dir.ok_or_else(|| usage.clone())?;

    let text = fs::read_to_string(&input)
        .map_err(|err| format!("Failed to read board log {input:?}: {err}"))?;
    let parsed = rust_plc::board_log::parse_board_log_text(&text)
        .map_err(|err| format!("Failed to parse board log: {err}"))?;

    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create output dir {out_dir:?}: {err}"))?;

    let mut board_trace_jsonl = String::new();
    for r in parsed.trace_rows {
        let mut line = serde_json::to_string(&r)
            .map_err(|err| format!("Failed to serialize trace row JSON: {err}"))?;
        line.push('\n');
        board_trace_jsonl.push_str(&line);
    }

    let board_trace_path = out_dir.join("board_trace.jsonl");
    fs::write(&board_trace_path, board_trace_jsonl)
        .map_err(|err| format!("Failed to write {board_trace_path:?}: {err}"))?;

    let tick_timing_jsonl = to_tick_timing_jsonl(&parsed.timing_rows)
        .map_err(|err| format!("Failed to serialize tick timing JSONL: {err}"))?;
    let tick_timing_path = out_dir.join("tick_timing.jsonl");
    fs::write(&tick_timing_path, tick_timing_jsonl)
        .map_err(|err| format!("Failed to write {tick_timing_path:?}: {err}"))?;

    Ok(())
}

#[derive(Debug, Serialize)]
struct CommissioningStepReport {
    id: &'static str,
    title: &'static str,
    command: String,
    status: &'static str,
    artifacts: Vec<String>,
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct CommissioningArtifacts {
    nominal_scenario: String,
    doctor_nominal: String,
    retain_config: String,
    retain_state: String,
    nominal_trace: String,
    gate_nominal_json: String,
    gate_nominal_dir: String,
    gate_nominal_diagnosis: String,
    fault_scenario: String,
    doctor_fault: String,
    online_force_script: String,
    online_var_script: String,
    online_var_bindings: String,
    fault_trace: String,
    online_force_audit: String,
    online_var_audit: String,
    gate_fault_json: String,
    gate_fault_dir: String,
    gate_fault_diagnosis: String,
}

#[derive(Debug, Serialize)]
struct CommissioningRunReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    status: &'static str,
    plc: String,
    out_dir: String,
    artifact_index: String,
    steps: Vec<CommissioningStepReport>,
    artifacts: CommissioningArtifacts,
}

fn commissioning_command_display(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        return program.to_string();
    }
    format!("{program} {}", args.join(" "))
}

fn run_commissioning_child(
    binary_path: &Path,
    args: &[String],
    stdout_capture: Option<&Path>,
) -> Result<(), String> {
    let output = Command::new(binary_path)
        .args(args)
        .output()
        .map_err(|err| format!("Failed to execute {}: {err}", binary_path.display()))?;

    if let Some(path) = stdout_capture {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create stdout capture directory {}: {err}",
                    parent.display()
                )
            })?;
        }
        fs::write(path, &output.stdout)
            .map_err(|err| format!("Failed to write stdout capture {}: {err}", path.display()))?;
    }

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr_trimmed = stderr.trim();
    if stderr_trimmed.is_empty() {
        Err(format!(
            "Command failed (status {:?}): {}",
            output.status.code(),
            args.join(" ")
        ))
    } else {
        Err(format!(
            "Command failed (status {:?}): {}\n{}",
            output.status.code(),
            args.join(" "),
            stderr_trimmed
        ))
    }
}

fn read_status_from_json(path: &Path) -> Result<String, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read JSON file {}: {err}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|err| format!("Failed to parse JSON file {}: {err}", path.display()))?;
    let status = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "JSON file {} is missing string field `status`",
                path.display()
            )
        })?;
    Ok(status.to_string())
}

fn commissioning_paths_to_relative(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| display_path_relative_to_cwd(p))
        .collect()
}

fn push_commissioning_step(
    steps: &mut Vec<CommissioningStepReport>,
    id: &'static str,
    title: &'static str,
    command: String,
    status: &'static str,
    artifacts: Vec<String>,
    detail: Option<String>,
) {
    steps.push(CommissioningStepReport {
        id,
        title,
        command,
        status,
        artifacts,
        detail,
    });
}

fn run_commissioning_step(
    steps: &mut Vec<CommissioningStepReport>,
    failure_reason: &mut Option<String>,
    program: &str,
    binary_path: &Path,
    id: &'static str,
    title: &'static str,
    cmd_args: Vec<String>,
    stdout_capture: Option<&Path>,
    artifact_paths: Vec<PathBuf>,
    checker: impl FnOnce() -> Result<(), String>,
) {
    let command = commissioning_command_display(program, &cmd_args);
    let artifacts_rel = commissioning_paths_to_relative(&artifact_paths);
    if failure_reason.is_some() {
        push_commissioning_step(
            steps,
            id,
            title,
            command,
            "skipped",
            artifacts_rel,
            Some("Skipped because an earlier commissioning step failed".to_string()),
        );
        return;
    }

    let result =
        run_commissioning_child(binary_path, &cmd_args, stdout_capture).and_then(|_| checker());
    match result {
        Ok(()) => push_commissioning_step(steps, id, title, command, "pass", artifacts_rel, None),
        Err(err) => {
            *failure_reason = Some(err.clone());
            push_commissioning_step(steps, id, title, command, "fail", artifacts_rel, Some(err));
        }
    }
}

fn run_commissioning_run_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "commissioning-run");
    let Some(plc_path_raw) = args.next() else {
        return Err(usage);
    };
    let plc_path = PathBuf::from(plc_path_raw);

    let mut out_dir: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out-dir <dir>".to_string()
                    })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for commissioning-run: {other}")),
        }
    }

    let out_dir = out_dir.ok_or_else(|| usage.clone())?;
    fs::create_dir_all(&out_dir).map_err(|err| {
        format!(
            "Failed to create commissioning output directory {}: {err}",
            out_dir.display()
        )
    })?;

    let binary_path = env::current_exe()
        .map_err(|err| format!("Failed to resolve current binary path: {err}"))?;

    let nominal_yaml = out_dir.join("nominal.yaml");
    let doctor_nominal_json = out_dir.join("doctor_nominal.json");
    let retain_toml = out_dir.join("retain.toml");
    let retain_state_json = out_dir.join("retain_state.json");
    let nominal_trace_jsonl = out_dir.join("nominal_trace.jsonl");
    let gate_nominal_dir = out_dir.join("gate_nominal");
    let gate_nominal_json = out_dir.join("gate_nominal.json");
    let gate_nominal_diagnosis = gate_nominal_dir.join("diagnosis_report.json");

    let fault_yaml = out_dir.join("fault.yaml");
    let doctor_fault_json = out_dir.join("doctor_fault.json");
    let online_force_jsonl = out_dir.join("online_force.jsonl");
    let online_var_jsonl = out_dir.join("online_var.jsonl");
    let online_var_bindings_toml = out_dir.join("online_var_bindings.toml");
    let fault_trace_jsonl = out_dir.join("fault_trace.jsonl");
    let online_force_audit_jsonl = out_dir.join("online_force_audit.jsonl");
    let online_var_audit_jsonl = out_dir.join("online_var_audit.jsonl");
    let gate_fault_dir = out_dir.join("gate_fault");
    let gate_fault_json = out_dir.join("gate_fault.json");
    let gate_fault_diagnosis = gate_fault_dir.join("diagnosis_report.json");
    let artifact_index_path = out_dir.join("commissioning_index.json");

    let artifacts = CommissioningArtifacts {
        nominal_scenario: display_path_relative_to_cwd(&nominal_yaml),
        doctor_nominal: display_path_relative_to_cwd(&doctor_nominal_json),
        retain_config: display_path_relative_to_cwd(&retain_toml),
        retain_state: display_path_relative_to_cwd(&retain_state_json),
        nominal_trace: display_path_relative_to_cwd(&nominal_trace_jsonl),
        gate_nominal_json: display_path_relative_to_cwd(&gate_nominal_json),
        gate_nominal_dir: display_path_relative_to_cwd(&gate_nominal_dir),
        gate_nominal_diagnosis: display_path_relative_to_cwd(&gate_nominal_diagnosis),
        fault_scenario: display_path_relative_to_cwd(&fault_yaml),
        doctor_fault: display_path_relative_to_cwd(&doctor_fault_json),
        online_force_script: display_path_relative_to_cwd(&online_force_jsonl),
        online_var_script: display_path_relative_to_cwd(&online_var_jsonl),
        online_var_bindings: display_path_relative_to_cwd(&online_var_bindings_toml),
        fault_trace: display_path_relative_to_cwd(&fault_trace_jsonl),
        online_force_audit: display_path_relative_to_cwd(&online_force_audit_jsonl),
        online_var_audit: display_path_relative_to_cwd(&online_var_audit_jsonl),
        gate_fault_json: display_path_relative_to_cwd(&gate_fault_json),
        gate_fault_dir: display_path_relative_to_cwd(&gate_fault_dir),
        gate_fault_diagnosis: display_path_relative_to_cwd(&gate_fault_diagnosis),
    };

    let mut steps: Vec<CommissioningStepReport> = Vec::new();
    let mut failure_reason: Option<String> = None;

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "A1",
        "Nominal scenario-init",
        vec![
            "scenario-init".to_string(),
            plc_path.display().to_string(),
            "--preset".to_string(),
            "normal".to_string(),
            "--out".to_string(),
            nominal_yaml.display().to_string(),
        ],
        None,
        vec![nominal_yaml.clone()],
        || {
            if nominal_yaml.exists() {
                Ok(())
            } else {
                Err(format!(
                    "Expected nominal scenario output {}",
                    nominal_yaml.display()
                ))
            }
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "A2",
        "Nominal scenario-doctor",
        vec![
            "scenario-doctor".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            nominal_yaml.display().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ],
        Some(&doctor_nominal_json),
        vec![doctor_nominal_json.clone()],
        || {
            let status = read_status_from_json(&doctor_nominal_json)?;
            if status == "pass" {
                Ok(())
            } else {
                Err(format!(
                    "doctor_nominal status must be `pass`, got `{status}`"
                ))
            }
        },
    );

    let retain_write_command = format!("write {}", retain_toml.display());
    if failure_reason.is_some() {
        push_commissioning_step(
            &mut steps,
            "A3",
            "Write retain config",
            retain_write_command,
            "skipped",
            vec![display_path_relative_to_cwd(&retain_toml)],
            Some("Skipped because an earlier commissioning step failed".to_string()),
        );
    } else {
        let retain_body = "schema_version = 1\n[digital_inputs]\ndi0 = false\n[digital_outputs]\ndo0 = false\n[analog_outputs]\nao0 = 0.0\n";
        let retain_result = fs::write(&retain_toml, retain_body).map_err(|err| {
            format!(
                "Failed to write retain config {}: {err}",
                retain_toml.display()
            )
        });
        match retain_result {
            Ok(()) => push_commissioning_step(
                &mut steps,
                "A3",
                "Write retain config",
                retain_write_command,
                "pass",
                vec![display_path_relative_to_cwd(&retain_toml)],
                None,
            ),
            Err(err) => {
                failure_reason = Some(err.clone());
                push_commissioning_step(
                    &mut steps,
                    "A3",
                    "Write retain config",
                    retain_write_command,
                    "fail",
                    vec![display_path_relative_to_cwd(&retain_toml)],
                    Some(err),
                );
            }
        }
    }

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "A4",
        "Nominal sim-plc with retain",
        vec![
            "sim-plc".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            nominal_yaml.display().to_string(),
            "--out".to_string(),
            nominal_trace_jsonl.display().to_string(),
            "--retain-config".to_string(),
            retain_toml.display().to_string(),
            "--retain-state".to_string(),
            retain_state_json.display().to_string(),
        ],
        None,
        vec![nominal_trace_jsonl.clone(), retain_state_json.clone()],
        || {
            if !nominal_trace_jsonl.exists() {
                return Err(format!(
                    "Expected nominal trace output {}",
                    nominal_trace_jsonl.display()
                ));
            }
            if !retain_state_json.exists() {
                return Err(format!(
                    "Expected retain state output {}",
                    retain_state_json.display()
                ));
            }
            Ok(())
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "A5",
        "Nominal no-board-gate",
        vec![
            "no-board-gate".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            nominal_yaml.display().to_string(),
            "--out-dir".to_string(),
            gate_nominal_dir.display().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ],
        Some(&gate_nominal_json),
        vec![
            gate_nominal_json.clone(),
            gate_nominal_dir.join("sil_trace.jsonl"),
            gate_nominal_dir.join("board_trace.jsonl"),
            gate_nominal_dir.join("diff_report.json"),
            gate_nominal_dir.join("timing_report.json"),
            gate_nominal_diagnosis.clone(),
        ],
        || {
            let status = read_status_from_json(&gate_nominal_json)?;
            if status != "pass" {
                return Err(format!(
                    "gate_nominal status must be `pass`, got `{status}`"
                ));
            }
            for required in [
                gate_nominal_dir.join("sil_trace.jsonl"),
                gate_nominal_dir.join("board_trace.jsonl"),
                gate_nominal_dir.join("diff_report.json"),
                gate_nominal_dir.join("timing_report.json"),
            ] {
                if !required.exists() {
                    return Err(format!(
                        "Missing nominal gate artifact {}",
                        required.display()
                    ));
                }
            }
            Ok(())
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "B1",
        "Fault scenario-init",
        vec![
            "scenario-init".to_string(),
            plc_path.display().to_string(),
            "--preset".to_string(),
            "sensor_stuck".to_string(),
            "--out".to_string(),
            fault_yaml.display().to_string(),
        ],
        None,
        vec![fault_yaml.clone()],
        || {
            if fault_yaml.exists() {
                Ok(())
            } else {
                Err(format!(
                    "Expected fault scenario output {}",
                    fault_yaml.display()
                ))
            }
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "B2",
        "Fault scenario-doctor",
        vec![
            "scenario-doctor".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            fault_yaml.display().to_string(),
            "--fix-preview".to_string(),
            "--output".to_string(),
            "json".to_string(),
        ],
        Some(&doctor_fault_json),
        vec![doctor_fault_json.clone()],
        || {
            let status = read_status_from_json(&doctor_fault_json)?;
            if status == "pass" {
                Ok(())
            } else {
                Err(format!(
                    "doctor_fault status must be `pass`, got `{status}`"
                ))
            }
        },
    );

    let scripts_write_command = format!(
        "write {}, {}, {}",
        online_force_jsonl.display(),
        online_var_jsonl.display(),
        online_var_bindings_toml.display()
    );
    if failure_reason.is_some() {
        push_commissioning_step(
            &mut steps,
            "B3",
            "Write online control scripts",
            scripts_write_command,
            "skipped",
            vec![
                display_path_relative_to_cwd(&online_force_jsonl),
                display_path_relative_to_cwd(&online_var_jsonl),
                display_path_relative_to_cwd(&online_var_bindings_toml),
            ],
            Some("Skipped because an earlier commissioning step failed".to_string()),
        );
    } else {
        let force_script = concat!(
            "{\"at_ms\":0,\"actor\":\"commissioning\",\"source\":\"panel\",\"channel\":\"DI0\",\"value\":true}\n",
            "{\"at_ms\":40,\"actor\":\"commissioning\",\"source\":\"panel\",\"channel\":\"DI0\",\"value\":null}\n",
        );
        let var_script = concat!(
            "{\"at_ms\":0,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"BOOL:diag_latch\",\"value\":true}\n",
            "{\"at_ms\":20,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"REAL:gain_k\",\"value\":1.25}\n",
            "{\"at_ms\":40,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"BOOL:diag_latch\",\"value\":null}\n",
            "{\"at_ms\":50,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"REAL:gain_k\",\"value\":null}\n",
        );
        let bindings_body = concat!(
            "schema_version = 1\n",
            "[bool]\n",
            "diag_latch = \"DI0\"\n",
            "[real]\n",
            "gain_k = \"AI0\"\n",
        );

        let write_result = fs::write(&online_force_jsonl, force_script)
            .and_then(|_| fs::write(&online_var_jsonl, var_script))
            .and_then(|_| fs::write(&online_var_bindings_toml, bindings_body))
            .map_err(|err| format!("Failed to write online control scripts: {err}"));

        match write_result {
            Ok(()) => push_commissioning_step(
                &mut steps,
                "B3",
                "Write online control scripts",
                scripts_write_command,
                "pass",
                vec![
                    display_path_relative_to_cwd(&online_force_jsonl),
                    display_path_relative_to_cwd(&online_var_jsonl),
                    display_path_relative_to_cwd(&online_var_bindings_toml),
                ],
                None,
            ),
            Err(err) => {
                failure_reason = Some(err.clone());
                push_commissioning_step(
                    &mut steps,
                    "B3",
                    "Write online control scripts",
                    scripts_write_command,
                    "fail",
                    vec![
                        display_path_relative_to_cwd(&online_force_jsonl),
                        display_path_relative_to_cwd(&online_var_jsonl),
                        display_path_relative_to_cwd(&online_var_bindings_toml),
                    ],
                    Some(err),
                );
            }
        }
    }

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "B4",
        "Fault sim-plc with online controls",
        vec![
            "sim-plc".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            fault_yaml.display().to_string(),
            "--out".to_string(),
            fault_trace_jsonl.display().to_string(),
            "--retain-config".to_string(),
            retain_toml.display().to_string(),
            "--retain-state".to_string(),
            retain_state_json.display().to_string(),
            "--enable-online-force-dev".to_string(),
            "--online-force-script".to_string(),
            online_force_jsonl.display().to_string(),
            "--online-force-audit-out".to_string(),
            online_force_audit_jsonl.display().to_string(),
            "--online-var-script".to_string(),
            online_var_jsonl.display().to_string(),
            "--online-var-bindings".to_string(),
            online_var_bindings_toml.display().to_string(),
            "--online-var-audit-out".to_string(),
            online_var_audit_jsonl.display().to_string(),
        ],
        None,
        vec![
            fault_trace_jsonl.clone(),
            online_force_audit_jsonl.clone(),
            online_var_audit_jsonl.clone(),
        ],
        || {
            for required in [
                fault_trace_jsonl.clone(),
                online_force_audit_jsonl.clone(),
                online_var_audit_jsonl.clone(),
            ] {
                if !required.exists() {
                    return Err(format!(
                        "Missing fault simulation artifact {}",
                        required.display()
                    ));
                }
            }
            Ok(())
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "B5",
        "Fault no-board-gate",
        vec![
            "no-board-gate".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            fault_yaml.display().to_string(),
            "--out-dir".to_string(),
            gate_fault_dir.display().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ],
        Some(&gate_fault_json),
        vec![
            gate_fault_json.clone(),
            gate_fault_dir.join("diff_report.json"),
            gate_fault_diagnosis.clone(),
        ],
        || {
            let status = read_status_from_json(&gate_fault_json)?;
            if status != "pass" {
                return Err(format!("gate_fault status must be `pass`, got `{status}`"));
            }
            let diff_report = gate_fault_dir.join("diff_report.json");
            if !diff_report.exists() {
                return Err(format!(
                    "Missing fault gate artifact {}",
                    diff_report.display()
                ));
            }
            Ok(())
        },
    );

    let report_status = if failure_reason.is_none() {
        "pass"
    } else {
        "fail"
    };

    let report = CommissioningRunReport {
        schema_version: 1,
        command: "commissioning-run",
        output: output_mode.as_str(),
        status: report_status,
        plc: display_path_relative_to_cwd(&plc_path),
        out_dir: display_path_relative_to_cwd(&out_dir),
        artifact_index: display_path_relative_to_cwd(&artifact_index_path),
        steps,
        artifacts,
    };

    let mut report_json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize commissioning index JSON: {err}"))?;
    report_json.push('\n');
    fs::write(&artifact_index_path, &report_json).map_err(|err| {
        format!(
            "Failed to write commissioning index {}: {err}",
            artifact_index_path.display()
        )
    })?;

    if output_mode == CliOutputMode::Json {
        print!("{report_json}");
    } else {
        eprintln!(
            "commissioning-run: {}",
            if report_status == "pass" {
                "PASS"
            } else {
                "FAIL"
            }
        );
        eprintln!(
            "  commissioning_index: {}",
            display_path_relative_to_cwd(&artifact_index_path)
        );
    }

    if let Some(reason) = failure_reason {
        return Err(format!(
            "commissioning-run failed: {reason} (index: {})",
            artifact_index_path.display()
        ));
    }

    Ok(())
}

