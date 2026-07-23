#[derive(Debug, Clone, Serialize)]
struct GateSummary {
    schema_version: u32,
    trace_match: bool,
    realtime_pass: bool,
    passed: bool,
    p99_exec_us: u64,
    overrun_count: u64,
    thresholds: RealtimeThresholdConfig,
    reasons: Vec<String>,
}


#[derive(Debug, Serialize)]
struct ReleaseBundleManifest<'a> {
    schema_version: u32,
    tool_version: &'a str,
    generated_at: &'a str,
    git_commit: &'a str,
    git_dirty: bool,
    artifacts: Vec<ReleaseBundleArtifact>,
}

#[derive(Debug, Serialize)]
struct ReleaseBundleArtifact {
    path: String,
    sha256: String,
    size_bytes: u64,
}

fn run_release_bundle_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "release-bundle");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut io_map_path: Option<PathBuf> = None;
    let mut max_p99_exec_us: Option<u64> = None;
    let mut max_overrun_count: Option<u64> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "--out-dir" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out-dir <dir>".to_string()
                    })?));
            }
            "--io-map" => {
                io_map_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --io-map <file>".to_string()
                    })?));
            }
            "--max-p99-exec-us" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --max-p99-exec-us <us>".to_string())?;
                max_p99_exec_us = Some(raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --max-p99-exec-us value (expected u64): {raw}")
                })?);
            }
            "--max-overrun-count" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --max-overrun-count <n>".to_string())?;
                max_overrun_count = Some(raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --max-overrun-count value (expected u64): {raw}")
                })?);
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for release-bundle: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_dir = out_dir.ok_or_else(|| usage.clone())?;

    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create out dir {out_dir:?}: {err}"))?;

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "Expected a .plc or .bundle.toml path, got: {plc_path}"
        ));
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let plc_source = loaded.source.clone();
    let plc_bytes = plc_source.as_bytes().to_vec();

    let plc_sha256 = sha256_hex(&plc_bytes);

    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&plc_source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "release-bundle", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let ir_bundle = compile_pipeline(&loaded).map_err(|errors| errors.join("\n\n"))?;

    // Board-oriented program generation uses 1ms ticks to align with firmware build artifacts.
    let compiled_board_program = state_machine_to_runtime_program(
        &ir_bundle.topology,
        &ir_bundle.constraints,
        &ir_bundle.state_machine,
        1,
    )
    .map_err(|err| format!("Failed to bridge to runtime Program: {err}"))?;
    let board_program = compiled_board_program.program();

    let usage = io_usage_for_program(board_program);
    let io_map = match io_map_path.as_ref() {
        None => None,
        Some(path) => {
            let toml_str = fs::read_to_string(&path)
                .map_err(|err| format!("Failed to read io map {path:?}: {err}"))?;
            let m = IoMap::from_toml_str(&toml_str)
                .map_err(|err| format!("Failed to parse io map TOML: {err}"))?;
            match m.validate_for_usage(usage) {
                Ok(()) => {}
                Err(IoMapError::MissingRequired { kind, id }) => {
                    return Err(format!(
                        "Invalid io map for this program: missing required mapping for {kind}{id}\n\
\n\
hint: the io map must contain a GPIO assignment for every DI/DO/AI/AO used by the program.\n\
Start from the generated `io_map.template.toml` under `--out-dir <dir>` and fill in GPIO numbers."
                    ));
                }
                Err(err) => {
                    return Err(format!("Invalid io map for this program: {err}"));
                }
            }
            Some(m)
        }
    };

    // Write/copy core bundle artifacts.
    let bundled_plc_path = out_dir.join("program.plc");
    fs::write(&bundled_plc_path, &plc_bytes)
        .map_err(|err| format!("Failed to write {bundled_plc_path:?}: {err}"))?;

    let bundled_scenario_path = out_dir.join("scenario.yaml");
    fs::write(&bundled_scenario_path, &scenario_yaml)
        .map_err(|err| format!("Failed to write {bundled_scenario_path:?}: {err}"))?;

    let io_map_template_path = out_dir.join("io_map.template.toml");
    let io_map_template = io_map_template_for_program(board_program);
    fs::write(&io_map_template_path, &io_map_template)
        .map_err(|err| format!("Failed to write {io_map_template_path:?}: {err}"))?;

    // Always include an io_map file in the bundle: either the user-provided map or a template.
    let bundled_io_map_path = out_dir.join("io_map.toml");
    if let Some(src) = io_map_path.as_ref() {
        fs::copy(src, &bundled_io_map_path).map_err(|err| {
            format!("Failed to copy io map {src:?} -> {bundled_io_map_path:?}: {err}")
        })?;
    } else {
        fs::write(&bundled_io_map_path, &io_map_template)
            .map_err(|err| format!("Failed to write {bundled_io_map_path:?}: {err}"))?;
    }

    let generated_program_path = out_dir.join("generated_program.rs");
    let mut generated_src = codegen::generate_program_module(board_program, "generated")
        .map_err(|err| format!("Codegen failed: {err:?}"))?;
    if !generated_src.ends_with('\n') {
        generated_src.push('\n');
    }
    fs::write(&generated_program_path, generated_src)
        .map_err(|err| format!("Failed to write {generated_program_path:?}: {err}"))?;

    let verification_report_path = out_dir.join("verification_report.json");
    let plc_path_text = PathBuf::from(&plc_path).to_string_lossy().to_string();
    write_verification_report(
        &plc_path_text,
        &verification_report_path,
        &ir_bundle.runtime_budget,
        &ir_bundle.verification,
    )?;

    // SIL artifacts for trace/report packaging.
    let compiled_sil_program = state_machine_to_runtime_program(
        &ir_bundle.topology,
        &ir_bundle.constraints,
        &ir_bundle.state_machine,
        scenario.tick_ms,
    )
    .map_err(|err| format!("Failed to bridge to SIL runtime Program: {err}"))?;
    let sil_program = compiled_sil_program.program();
    let sil_trace_path = out_dir.join("sil_trace.jsonl");
    let sim_report_path = out_dir.join("sim_report.json");
    let (num_di, num_do, num_ai, num_ao) =
        io_sizes_for_program_and_scenario(sil_program, &scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    let run = sim::run_program_for_scenario(sil_program, &scenario, &mut io)
        .map_err(|err| format!("SIL simulation failed: {err}"))?;
    fs::write(&sil_trace_path, run.trace.into_string())
        .map_err(|err| format!("Failed to write trace file {sil_trace_path:?}: {err}"))?;
    let mut sim_report_json = serde_json::to_string_pretty(&run.report)
        .map_err(|err| format!("Failed to serialize sim report JSON: {err}"))?;
    sim_report_json.push('\n');
    fs::write(&sim_report_path, sim_report_json)
        .map_err(|err| format!("Failed to write sim report {sim_report_path:?}: {err}"))?;

    let (_board_log_path, board_trace_path, _board_meta_path, tick_timing_path) =
        write_virtual_board_artifacts(
            Path::new(&plc_path),
            &scenario_path,
            sil_program,
            &scenario,
            &out_dir,
        )?;

    let board_trace_text = fs::read_to_string(&board_trace_path)
        .map_err(|err| format!("Failed to read board trace {board_trace_path:?}: {err}"))?;
    let sil_trace_text = fs::read_to_string(&sil_trace_path)
        .map_err(|err| format!("Failed to read SIL trace {sil_trace_path:?}: {err}"))?;
    let sil_events = rust_plc::trace_diff::parse_trace_jsonl(&sil_trace_text)
        .map_err(|err| format!("Failed to parse SIL trace JSONL: {err}"))?;
    let board_events = rust_plc::trace_diff::parse_trace_jsonl(&board_trace_text)
        .map_err(|err| format!("Failed to parse board trace JSONL: {err}"))?;
    let diff_report = rust_plc::trace_diff::diff_traces(&sil_events, &board_events, 3);
    let diff_report_path = out_dir.join("diff_report.json");
    let mut diff_json = serde_json::to_string_pretty(&diff_report)
        .map_err(|err| format!("Failed to serialize diff report JSON: {err}"))?;
    diff_json.push('\n');
    fs::write(&diff_report_path, diff_json)
        .map_err(|err| format!("Failed to write diff report {diff_report_path:?}: {err}"))?;

    let tick_timing_text = fs::read_to_string(&tick_timing_path)
        .map_err(|err| format!("Failed to read tick timing {tick_timing_path:?}: {err}"))?;
    let tick_timing_rows = parse_tick_timing_jsonl(&tick_timing_text)
        .map_err(|err| format!("Failed to parse tick timing JSONL: {err}"))?;
    let timing_report = build_timing_report(&tick_timing_rows)
        .ok_or_else(|| "tick_timing.jsonl is empty; cannot build timing report".to_string())?;
    let timing_report_path = out_dir.join("timing_report.json");
    let mut timing_json = serde_json::to_string_pretty(&timing_report)
        .map_err(|err| format!("Failed to serialize timing report JSON: {err}"))?;
    timing_json.push('\n');
    fs::write(&timing_report_path, timing_json)
        .map_err(|err| format!("Failed to write timing report {timing_report_path:?}: {err}"))?;

    let mut gate_reasons = Vec::new();
    let mut realtime_pass = true;
    if !diff_report.is_match {
        gate_reasons.push(format!(
            "trace mismatch (tick={:?}, type={:?}, index={:?})",
            diff_report.first_mismatch_tick, diff_report.mismatch_type, diff_report.mismatch_index
        ));
    }
    if let Some(limit) = max_p99_exec_us {
        if timing_report.exec_us_p99 > limit {
            realtime_pass = false;
            gate_reasons.push(format!(
                "p99 exec_us={} exceeds threshold {}",
                timing_report.exec_us_p99, limit
            ));
        }
    }
    if let Some(limit) = max_overrun_count {
        if timing_report.overrun_count > limit {
            realtime_pass = false;
            gate_reasons.push(format!(
                "overrun_count={} exceeds threshold {}",
                timing_report.overrun_count, limit
            ));
        }
    }
    let gate_summary = GateSummary {
        schema_version: 1,
        trace_match: diff_report.is_match,
        realtime_pass,
        passed: gate_reasons.is_empty(),
        p99_exec_us: timing_report.exec_us_p99,
        overrun_count: timing_report.overrun_count,
        thresholds: RealtimeThresholdConfig {
            max_p99_exec_us,
            max_overrun_count,
        },
        reasons: gate_reasons,
    };
    let gate_summary_path = out_dir.join("gate_summary.json");
    let mut gate_summary_json = serde_json::to_string_pretty(&gate_summary)
        .map_err(|err| format!("Failed to serialize gate summary JSON: {err}"))?;
    gate_summary_json.push('\n');
    fs::write(&gate_summary_path, gate_summary_json)
        .map_err(|err| format!("Failed to write gate summary {gate_summary_path:?}: {err}"))?;

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let git_metadata = detect_git_metadata();

    let build_meta_path = out_dir.join("build_meta.json");
    let meta = BuildMeta {
        plc_sha256: &plc_sha256,
        generated_at: &generated_at,
        tool_version: env!("CARGO_PKG_VERSION"),
        runtime_semver: runtime_core::VERSION,
        git_commit: &git_metadata.commit,
        git_dirty: git_metadata.dirty,
        runtime_budget: ir_bundle.runtime_budget.summary(),
        realtime_profile: RealtimeProfile {
            tick_ms: scenario.tick_ms,
            thresholds: RealtimeThresholdConfig {
                max_p99_exec_us,
                max_overrun_count,
            },
            overrun_count: timing_report.overrun_count,
            p99_exec_us: timing_report.exec_us_p99,
        },
        io_map,
    };
    let mut meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("Failed to serialize build_meta.json: {err}"))?;
    meta_json.push('\n');
    fs::write(&build_meta_path, meta_json)
        .map_err(|err| format!("Failed to write {build_meta_path:?}: {err}"))?;

    let manifest_path = out_dir.join("manifest.json");
    let mut artifact_paths: Vec<PathBuf> = vec![
        bundled_plc_path,
        bundled_scenario_path,
        bundled_io_map_path,
        io_map_template_path,
        generated_program_path,
        verification_report_path,
        sil_trace_path,
        sim_report_path,
        tick_timing_path,
        timing_report_path,
        gate_summary_path,
        diff_report_path,
        build_meta_path,
    ];
    artifact_paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let mut artifacts = Vec::new();
    for p in &artifact_paths {
        let rel = p
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Non-utf8 artifact filename: {p:?}"))?
            .to_string();
        let (sha, size) = sha256_file(p)?;
        artifacts.push(ReleaseBundleArtifact {
            path: rel,
            sha256: sha,
            size_bytes: size,
        });
    }

    let manifest = ReleaseBundleManifest {
        schema_version: 1,
        tool_version: env!("CARGO_PKG_VERSION"),
        generated_at: &generated_at,
        git_commit: &git_metadata.commit,
        git_dirty: git_metadata.dirty,
        artifacts,
    };
    let mut manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("Failed to serialize manifest.json: {err}"))?;
    manifest_json.push('\n');
    fs::write(&manifest_path, manifest_json)
        .map_err(|err| format!("Failed to write {manifest_path:?}: {err}"))?;

    Ok(())
}
