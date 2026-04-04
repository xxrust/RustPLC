fn run_pil_run_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "pil-run");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for pil-run: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&loaded.source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "pil-run", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let program = compile_plc_to_runtime_program(&loaded.source, scenario.tick_ms)?;

    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(&program, &scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    scenario
        .apply_to_simio(&mut io)
        .map_err(|e| format!("scenario apply failed: {e}"))?;

    let mut rt =
        runtime_core::Runtime::new(&program).map_err(|e| format!("runtime init failed: {e:?}"))?;

    println!("boot ok");
    for _ in 0..scenario.duration_ticks() {
        let tick = io.tick().0;
        let ts_ms = tick.saturating_mul(scenario.tick_ms);
        println!("TICK tick={tick} ts_ms={ts_ms}");

        rt.tick_with_trace_and_logs(
            &mut io,
            |e| {
                let ts_ms = e.tick.0.saturating_mul(scenario.tick_ms);
                println!(
                    "TRACE tick={} task={} from={} to={} reason={} ts_ms={}",
                    e.tick.0,
                    e.task,
                    e.from.0,
                    e.to.0,
                    reason_str(e.reason),
                    ts_ms
                );
            },
            |log| {
                let ts_ms = log.tick.0.saturating_mul(scenario.tick_ms);
                println!(
                    "LOG tick={} task={} step={} msg_id={} msg={} ts_ms={}",
                    log.tick.0, log.task, log.step.0, log.message_id, log.message, ts_ms
                );
            },
        )
        .map_err(|e| format!("runtime tick failed: {e:?}"))?;

        if is_halted(&rt, &program) {
            break;
        }
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct VirtualBoardMeta<'a> {
    schema_version: u32,
    source_plc: &'a str,
    scenario_path: &'a str,
    generated_at: &'a str,
    tick_ms: u64,
    duration_ticks: u64,
}

fn write_virtual_board_artifacts(
    plc_path: &Path,
    scenario_path: &Path,
    program: &Program<'_>,
    scenario: &sim::Scenario,
    out_dir: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), String> {
    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(program, scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    scenario
        .apply_to_simio(&mut io)
        .map_err(|e| format!("scenario apply failed: {e}"))?;
    let mut rt =
        runtime_core::Runtime::new(program).map_err(|e| format!("runtime init failed: {e:?}"))?;
    let tick_period_us = scenario.tick_ms.saturating_mul(1000);

    let board_log = std::cell::RefCell::new(String::new());
    board_log.borrow_mut().push_str("boot ok\n");
    let mut tick_timing_rows: Vec<TickTimingSample> = Vec::new();

    for _ in 0..scenario.duration_ticks() {
        let tick = io.tick().0;
        let ts_ms = tick.saturating_mul(scenario.tick_ms);
        let ts_start_us = tick.saturating_mul(tick_period_us);
        let transition_count = std::cell::Cell::new(0u64);
        let log_count = std::cell::Cell::new(0u64);
        board_log
            .borrow_mut()
            .push_str(&format!("TICK tick={tick} ts_ms={ts_ms}\n"));

        rt.tick_with_trace_and_logs(
            &mut io,
            |e| {
                transition_count.set(transition_count.get().saturating_add(1));
                let ts_ms = e.tick.0.saturating_mul(scenario.tick_ms);
                board_log.borrow_mut().push_str(&format!(
                    "TRACE tick={} task={} from={} to={} reason={} ts_ms={}\n",
                    e.tick.0,
                    e.task,
                    e.from.0,
                    e.to.0,
                    reason_str(e.reason),
                    ts_ms
                ));
            },
            |log| {
                log_count.set(log_count.get().saturating_add(1));
                let ts_ms = log.tick.0.saturating_mul(scenario.tick_ms);
                board_log.borrow_mut().push_str(&format!(
                    "LOG tick={} task={} step={} msg_id={} msg={} ts_ms={}\n",
                    log.tick.0, log.task, log.step.0, log.message_id, log.message, ts_ms
                ));
            },
        )
        .map_err(|e| format!("runtime tick failed: {e:?}"))?;

        // Keep virtual-board timing deterministic for stable no-board regressions.
        let exec_us = transition_count
            .get()
            .saturating_mul(40)
            .saturating_add(log_count.get().saturating_mul(15))
            .saturating_add(10);
        let overrun = exec_us > tick_period_us;
        let slack_us = if overrun {
            0
        } else {
            tick_period_us.saturating_sub(exec_us)
        };
        let ts_end_us = ts_start_us.saturating_add(exec_us);
        tick_timing_rows.push(TickTimingSample {
            tick,
            ts_start_us,
            ts_end_us,
            exec_us,
            slack_us,
            overrun,
        });
        board_log.borrow_mut().push_str(&format!(
            "TIMING tick={tick} ts_start_us={ts_start_us} ts_end_us={ts_end_us} exec_us={exec_us} slack_us={slack_us} overrun={overrun}\n"
        ));

        if is_halted(&rt, program) {
            break;
        }
    }

    let board_log = board_log.into_inner();
    let board_log_path = out_dir.join("board.log");
    fs::write(&board_log_path, &board_log)
        .map_err(|err| format!("Failed to write board log {board_log_path:?}: {err}"))?;

    let rows = rust_plc::board_trace::parse_trace_text(&board_log)
        .map_err(|err| format!("Failed to parse generated board trace: {err}"))?;
    let mut board_trace_jsonl = String::new();
    for row in rows {
        let mut line = serde_json::to_string(&row)
            .map_err(|err| format!("Failed to serialize trace row: {err}"))?;
        line.push('\n');
        board_trace_jsonl.push_str(&line);
    }
    let board_trace_path = out_dir.join("board_trace.jsonl");
    fs::write(&board_trace_path, board_trace_jsonl)
        .map_err(|err| format!("Failed to write board trace {board_trace_path:?}: {err}"))?;

    let tick_timing_jsonl = to_tick_timing_jsonl(&tick_timing_rows)
        .map_err(|err| format!("Failed to serialize tick timing JSONL: {err}"))?;
    let tick_timing_path = out_dir.join("tick_timing.jsonl");
    fs::write(&tick_timing_path, tick_timing_jsonl)
        .map_err(|err| format!("Failed to write tick timing {tick_timing_path:?}: {err}"))?;

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let plc_path_text = plc_path.to_string_lossy().to_string();
    let scenario_path_text = scenario_path.to_string_lossy().to_string();
    let meta = VirtualBoardMeta {
        schema_version: 1,
        source_plc: &plc_path_text,
        scenario_path: &scenario_path_text,
        generated_at: &generated_at,
        tick_ms: scenario.tick_ms,
        duration_ticks: scenario.duration_ticks(),
    };
    let mut meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("Failed to serialize virtual board meta JSON: {err}"))?;
    meta_json.push('\n');
    let meta_path = out_dir.join("virtual_board_meta.json");
    fs::write(&meta_path, meta_json)
        .map_err(|err| format!("Failed to write virtual board meta {meta_path:?}: {err}"))?;

    Ok((
        board_log_path,
        board_trace_path,
        meta_path,
        tick_timing_path,
    ))
}

fn run_virtual_board_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "virtual-board");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
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
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for virtual-board: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_dir = out_dir.ok_or_else(|| usage.clone())?;
    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create output directory {out_dir:?}: {err}"))?;

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&loaded.source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "virtual-board", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let program = compile_plc_to_runtime_program(&loaded.source, scenario.tick_ms)?;
    write_virtual_board_artifacts(
        Path::new(&plc_path),
        &scenario_path,
        &program,
        &scenario,
        &out_dir,
    )?;

    Ok(())
}

fn reason_str(r: runtime_core::TransitionReason) -> &'static str {
    match r {
        runtime_core::TransitionReason::Action => "action",
        runtime_core::TransitionReason::DelayElapsed => "delay_elapsed",
        runtime_core::TransitionReason::WaitSatisfied => "wait_satisfied",
        runtime_core::TransitionReason::Timeout => "timeout",
        runtime_core::TransitionReason::Goto => "goto",
    }
}
