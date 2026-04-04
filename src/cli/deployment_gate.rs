fn run_no_board_gate_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "no-board-gate");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut sil_scenario_path: Option<PathBuf> = None;
    let mut board_scenario_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut context_window: usize = 3;
    let mut max_p99_exec_us: Option<u64> = None;
    let mut max_overrun_count: Option<u64> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "--sil-scenario" => {
                sil_scenario_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --sil-scenario <scenario.yaml>".to_string()
                })?));
            }
            "--board-scenario" => {
                board_scenario_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --board-scenario <scenario.yaml>".to_string()
                })?));
            }
            "--out-dir" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out-dir <dir>".to_string()
                    })?));
            }
            "--context" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --context <n>".to_string())?;
                context_window = raw
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --context value (expected usize): {raw}"))?;
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
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for no-board-gate: {other}")),
        }
    }

    let out_dir = out_dir.ok_or_else(|| usage.clone())?;

    let sil_scenario_path = sil_scenario_path
        .or_else(|| scenario_path.clone())
        .ok_or_else(|| usage.clone())?;
    let board_scenario_path = board_scenario_path
        .or_else(|| scenario_path.clone())
        .ok_or_else(|| usage.clone())?;

    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create output directory {out_dir:?}: {err}"))?;

    let loaded = load_plc_source(Path::new(&plc_path))?;

    let sil_yaml = read_scenario_yaml_file(&sil_scenario_path)?;
    let board_yaml = read_scenario_yaml_file(&board_scenario_path)?;
    let sil_yaml = resolve_scenario_yaml_for_plc(&loaded.source, &sil_yaml).map_err(|e| {
        format_resolve_scenario_yaml_error(&plc_path, &sil_scenario_path, "no-board-gate", &e)
    })?;
    let board_yaml = resolve_scenario_yaml_for_plc(&loaded.source, &board_yaml).map_err(|e| {
        format_resolve_scenario_yaml_error(&plc_path, &board_scenario_path, "no-board-gate", &e)
    })?;

    let sil_scenario = parse_scenario_yaml(&sil_yaml)?;
    let board_scenario = parse_scenario_yaml(&board_yaml)?;

    if sil_scenario.tick_ms != board_scenario.tick_ms {
        return Err(format!(
            "SIL tick_ms ({}) must match board tick_ms ({}) for no-board-gate",
            sil_scenario.tick_ms, board_scenario.tick_ms
        ));
    }

    let program = compile_plc_to_runtime_program(&loaded.source, sil_scenario.tick_ms)?;

    let sil_trace_path = out_dir.join("sil_trace.jsonl");
    let (num_di, num_do, num_ai, num_ao) =
        io_sizes_for_program_and_scenario(&program, &sil_scenario);
    let mut sil_io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    let sil_run =
        sim::run_program_for_scenario(&program, &sil_scenario, &mut sil_io).map_err(|e| {
            let mut msg = format!("SIL simulation failed: {e}");
            if let Some(hint) = scenario_mismatch_hint_for_example(
                &plc_path,
                &sil_scenario_path,
                &e,
                "no-board-gate",
            ) {
                msg.push_str("\n\n");
                msg.push_str(&hint);
            }
            msg
        })?;

    fs::write(&sil_trace_path, sil_run.trace.into_string())
        .map_err(|err| format!("Failed to write SIL trace file {sil_trace_path:?}: {err}"))?;

    let (_, board_trace_path, _, tick_timing_path) = write_virtual_board_artifacts(
        Path::new(&plc_path),
        &board_scenario_path,
        &program,
        &board_scenario,
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

    let report = rust_plc::trace_diff::diff_traces(&sil_events, &board_events, context_window);
    let diff_report_path = out_dir.join("diff_report.json");
    let mut json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize diff report JSON: {err}"))?;
    json.push('\n');
    fs::write(&diff_report_path, json)
        .map_err(|err| format!("Failed to write diff report {diff_report_path:?}: {err}"))?;

    let tick_timing_text = fs::read_to_string(&tick_timing_path)
        .map_err(|err| format!("Failed to read tick timing {tick_timing_path:?}: {err}"))?;
    let tick_timing_rows = parse_tick_timing_jsonl(&tick_timing_text)
        .map_err(|err| format!("Failed to parse tick timing JSONL: {err}"))?;
    let timing_report = build_timing_report(&tick_timing_rows)
        .ok_or_else(|| "tick_timing.jsonl is empty; cannot evaluate realtime gate".to_string())?;
    let timing_report_path = out_dir.join("timing_report.json");
    let mut timing_json = serde_json::to_string_pretty(&timing_report)
        .map_err(|err| format!("Failed to serialize timing report JSON: {err}"))?;
    timing_json.push('\n');
    fs::write(&timing_report_path, timing_json)
        .map_err(|err| format!("Failed to write timing report {timing_report_path:?}: {err}"))?;

    let mut realtime_failures = Vec::new();
    if let Some(limit) = max_p99_exec_us {
        if timing_report.exec_us_p99 > limit {
            realtime_failures.push(format!(
                "p99 exec_us={} exceeds --max-p99-exec-us={limit}",
                timing_report.exec_us_p99
            ));
        }
    }
    if let Some(limit) = max_overrun_count {
        if timing_report.overrun_count > limit {
            realtime_failures.push(format!(
                "overrun_count={} exceeds --max-overrun-count={limit}",
                timing_report.overrun_count
            ));
        }
    }

    let gate_failed = !report.is_match || !realtime_failures.is_empty();
    let diagnosis_report_path = out_dir.join("diagnosis_report.json");
    let mut diagnosis_report_rel: Option<String> = None;
    let mut diagnosis_top_candidate_code: Option<String> = None;
    let mut diagnosis_evidence_source: Option<String> = None;

    if gate_failed {
        let diagnosis = diagnose(DiagnosisInput {
            plc_source: &loaded.source,
            scenario: &sil_scenario,
            trace_events: Some(&sil_events),
            diff_report: Some(&report),
            timing_report: Some(&timing_report),
            evidence_source: EvidenceSource::NoBoard,
            io_snapshot: None,
        })
        .map_err(|err| format!("Failed to build no-board diagnosis report: {err}"))?;
        diagnosis_top_candidate_code = diagnosis
            .candidates
            .first()
            .map(|candidate| candidate.issue_code.clone());
        diagnosis_evidence_source =
            Some(evidence_source_label(EvidenceSource::NoBoard).to_string());
        let mut diagnosis_json = serde_json::to_string_pretty(&diagnosis)
            .map_err(|err| format!("Failed to serialize diagnosis report JSON: {err}"))?;
        diagnosis_json.push('\n');
        fs::write(&diagnosis_report_path, diagnosis_json).map_err(|err| {
            format!(
                "Failed to write diagnosis report {}: {err}",
                diagnosis_report_path.display()
            )
        })?;
        diagnosis_report_rel = Some(display_path_relative_to_cwd(&diagnosis_report_path));
    }

    if output_mode == CliOutputMode::Human {
        if report.is_match {
            eprintln!(
                "no-board-gate: PASS (sil_events={}, board_events={})",
                report.sil_events, report.board_events
            );
        } else {
            eprintln!(
                "no-board-gate: FAIL (tick={:?}, type={:?}, index={:?})",
                report.first_mismatch_tick, report.mismatch_type, report.mismatch_index
            );
        }
        eprintln!("  sil_trace: {}", sil_trace_path.display());
        eprintln!("  board_trace: {}", board_trace_path.display());
        eprintln!("  diff_report: {}", diff_report_path.display());
        eprintln!(
            "  timing_report: {} (p99_exec_us={}, overrun_count={})",
            timing_report_path.display(),
            timing_report.exec_us_p99,
            timing_report.overrun_count
        );

        for reason in &realtime_failures {
            eprintln!("  realtime-gate: {reason}");
        }
        if let Some(path) = &diagnosis_report_rel {
            eprintln!("  diagnosis_report: {path}");
        }
    } else {
        #[derive(Serialize)]
        struct NoBoardGateJson<'a> {
            schema_version: u32,
            command: &'static str,
            output: &'static str,
            status: &'static str,
            trace_match: bool,
            realtime_failures: &'a [String],
            sil_trace: String,
            board_trace: String,
            diff_report: String,
            timing_report: String,
            p99_exec_us: u64,
            overrun_count: u64,
            diagnosis_report: Option<String>,
            diagnosis_top_candidate_code: Option<String>,
            diagnosis_evidence_source: Option<String>,
        }
        let payload = NoBoardGateJson {
            schema_version: 2,
            command: "no-board-gate",
            output: output_mode.as_str(),
            status: if report.is_match && realtime_failures.is_empty() {
                "pass"
            } else {
                "fail"
            },
            trace_match: report.is_match,
            realtime_failures: &realtime_failures,
            sil_trace: display_path_relative_to_cwd(&sil_trace_path),
            board_trace: display_path_relative_to_cwd(&board_trace_path),
            diff_report: display_path_relative_to_cwd(&diff_report_path),
            timing_report: display_path_relative_to_cwd(&timing_report_path),
            p99_exec_us: timing_report.exec_us_p99,
            overrun_count: timing_report.overrun_count,
            diagnosis_report: diagnosis_report_rel.clone(),
            diagnosis_top_candidate_code: diagnosis_top_candidate_code.clone(),
            diagnosis_evidence_source: diagnosis_evidence_source.clone(),
        };
        let mut json = serde_json::to_string_pretty(&payload)
            .map_err(|err| format!("Failed to serialize no-board-gate JSON output: {err}"))?;
        json.push('\n');
        print!("{json}");
    }

    if !report.is_match || !realtime_failures.is_empty() {
        let mut reasons = Vec::new();
        if !report.is_match {
            reasons.push(format!(
                "trace mismatch (see {})",
                diff_report_path.display()
            ));
        }
        if !realtime_failures.is_empty() {
            reasons.push(format!(
                "realtime threshold exceeded ({})",
                realtime_failures.join("; ")
            ));
        }
        return Err(format!("no-board-gate failed: {}", reasons.join(", ")));
    }
    Ok(())
}

