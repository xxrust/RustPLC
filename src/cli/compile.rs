use crate::cli::shared::compile_pipeline::{
    IrBundle, RuntimeBudget, compile_pipeline, compile_pipeline_with_reusable_verification,
    reusable_verification_from_ir_bundle, write_verification_report,
};
use crate::cli_support::help::{print_command_help_and_exit, print_usage};
use rust_plc::source_bundle::{is_supported_plc_source_path, load_plc_source, plc_source_stem};
use rust_plc::verification::{VerificationSummary, WarningEntry, WarningLevel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const IR_CACHE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeBudgetThresholds {
    max_actions_per_transition: usize,
    max_actions_per_tick_upper_bound: usize,
    max_parallel_branches: usize,
    max_race_branches: usize,
    warn_on_same_tick_cycle: bool,
    action_cost_us: u64,
    transition_cost_us: u64,
    parallel_expand_cost_us: u64,
    max_budget_time_estimate_us: u64,
}

impl Default for RuntimeBudgetThresholds {
    fn default() -> Self {
        Self {
            max_actions_per_transition: 16,
            max_actions_per_tick_upper_bound: 512,
            max_parallel_branches: 8,
            max_race_branches: 8,
            warn_on_same_tick_cycle: true,
            action_cost_us: 8,
            transition_cost_us: 5,
            parallel_expand_cost_us: 12,
            max_budget_time_estimate_us: 2_000,
        }
    }
}

impl RuntimeBudgetThresholds {
    fn from_env() -> Self {
        let mut out = Self::default();
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_ACTIONS_PER_TRANSITION") {
            if let Ok(n) = v.parse::<usize>() {
                out.max_actions_per_transition = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_ACTIONS_PER_TICK") {
            if let Ok(n) = v.parse::<usize>() {
                out.max_actions_per_tick_upper_bound = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_PARALLEL_BRANCHES") {
            if let Ok(n) = v.parse::<usize>() {
                out.max_parallel_branches = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_RACE_BRANCHES") {
            if let Ok(n) = v.parse::<usize>() {
                out.max_race_branches = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_WARN_ON_SAME_TICK_CYCLE") {
            if let Ok(b) = v.parse::<bool>() {
                out.warn_on_same_tick_cycle = b;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_ACTION_COST_US") {
            if let Ok(n) = v.parse::<u64>() {
                out.action_cost_us = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_TRANSITION_COST_US") {
            if let Ok(n) = v.parse::<u64>() {
                out.transition_cost_us = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_PARALLEL_EXPAND_COST_US") {
            if let Ok(n) = v.parse::<u64>() {
                out.parallel_expand_cost_us = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_TIME_ESTIMATE_US") {
            if let Ok(n) = v.parse::<u64>() {
                out.max_budget_time_estimate_us = n;
            }
        }
        out
    }
}

pub(super) fn run_compile_command(program: String, first: String, remaining: Vec<String>) {
    let path = first;
    let mut args = remaining.into_iter();
    let mut report_path: Option<PathBuf> = None;
    let mut no_print_ir = false;
    let mut ir_out_path: Option<PathBuf> = None;
    let mut cache_dir: Option<PathBuf> = None;
    let mut deny_warnings = false;
    let mut budget_thresholds = RuntimeBudgetThresholds::from_env();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--report" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --report <file>");
                    std::process::exit(1);
                });
                report_path = Some(PathBuf::from(value));
            }
            "--no-print-ir" => {
                no_print_ir = true;
            }
            "--ir-out" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --ir-out <file>");
                    std::process::exit(1);
                });
                ir_out_path = Some(PathBuf::from(value));
            }
            "--cache-dir" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --cache-dir <dir>");
                    std::process::exit(1);
                });
                cache_dir = Some(PathBuf::from(value));
            }
            "--deny-warnings" => {
                deny_warnings = true;
            }
            "--budget-max-actions-per-transition" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-actions-per-transition <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_actions_per_transition =
                    value.parse::<usize>().unwrap_or_else(|_| {
                        eprintln!(
                            "Invalid integer for --budget-max-actions-per-transition: {value}"
                        );
                        std::process::exit(1);
                    });
            }
            "--budget-max-actions-per-tick" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-actions-per-tick <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_actions_per_tick_upper_bound =
                    value.parse::<usize>().unwrap_or_else(|_| {
                        eprintln!("Invalid integer for --budget-max-actions-per-tick: {value}");
                        std::process::exit(1);
                    });
            }
            "--budget-max-parallel-branches" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-parallel-branches <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_parallel_branches =
                    value.parse::<usize>().unwrap_or_else(|_| {
                        eprintln!("Invalid integer for --budget-max-parallel-branches: {value}");
                        std::process::exit(1);
                    });
            }
            "--budget-max-race-branches" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-race-branches <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_race_branches = value.parse::<usize>().unwrap_or_else(|_| {
                    eprintln!("Invalid integer for --budget-max-race-branches: {value}");
                    std::process::exit(1);
                });
            }
            "--budget-warn-on-same-tick-cycle" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-warn-on-same-tick-cycle <true|false>");
                    std::process::exit(1);
                });
                budget_thresholds.warn_on_same_tick_cycle =
                    value.parse::<bool>().unwrap_or_else(|_| {
                        eprintln!("Invalid boolean for --budget-warn-on-same-tick-cycle: {value}");
                        std::process::exit(1);
                    });
            }
            "--budget-action-cost-us" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-action-cost-us <n>");
                    std::process::exit(1);
                });
                budget_thresholds.action_cost_us = value.parse::<u64>().unwrap_or_else(|_| {
                    eprintln!("Invalid integer for --budget-action-cost-us: {value}");
                    std::process::exit(1);
                });
            }
            "--budget-transition-cost-us" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-transition-cost-us <n>");
                    std::process::exit(1);
                });
                budget_thresholds.transition_cost_us = value.parse::<u64>().unwrap_or_else(|_| {
                    eprintln!("Invalid integer for --budget-transition-cost-us: {value}");
                    std::process::exit(1);
                });
            }
            "--budget-parallel-expand-cost-us" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-parallel-expand-cost-us <n>");
                    std::process::exit(1);
                });
                budget_thresholds.parallel_expand_cost_us =
                    value.parse::<u64>().unwrap_or_else(|_| {
                        eprintln!("Invalid integer for --budget-parallel-expand-cost-us: {value}");
                        std::process::exit(1);
                    });
            }
            "--budget-max-time-estimate-us" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-time-estimate-us <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_budget_time_estimate_us =
                    value.parse::<u64>().unwrap_or_else(|_| {
                        eprintln!("Invalid integer for --budget-max-time-estimate-us: {value}");
                        std::process::exit(1);
                    });
            }
            "-h" | "--help" => {
                print_command_help_and_exit(&program, "compile", 0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_usage(&program);
                std::process::exit(1);
            }
        }
    }

    if !is_supported_plc_source_path(Path::new(&path)) {
        eprintln!("Expected a .plc or .bundle.toml path, got: {path}");
        std::process::exit(1);
    }

    let loaded = match load_plc_source(Path::new(&path)) {
        Ok(contents) => contents,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let mut ir_bundle = match compile_pipeline_maybe_cached(&loaded, cache_dir.as_deref()) {
        Ok(ir_bundle) => ir_bundle,
        Err(errors) => {
            for (index, error) in errors.iter().enumerate() {
                if index > 0 {
                    eprintln!();
                }
                eprintln!("{error}");
            }
            std::process::exit(1);
        }
    };
    apply_runtime_budget_warnings(
        &mut ir_bundle.verification,
        &mut ir_bundle.runtime_budget,
        budget_thresholds,
    );

    let report_path =
        report_path.unwrap_or_else(|| default_verification_report_path(Path::new(&path)));
    if let Err(err) = write_verification_report(
        &path,
        &report_path,
        &ir_bundle.runtime_budget,
        &ir_bundle.verification,
    ) {
        eprintln!("{err}");
        std::process::exit(1);
    }

    print_success_summary(&ir_bundle.verification);
    eprintln!("verification_report: {}", report_path.display());
    if deny_warnings {
        let blocking_warnings = collect_blocking_warnings(&ir_bundle.verification);
        if !blocking_warnings.is_empty() {
            eprintln!("--deny-warnings blocked due to verification warnings");
            for warning in blocking_warnings {
                eprintln!("  - {warning}");
            }
            std::process::exit(2);
        }
    }

    if let Some(ir_out_path) = ir_out_path {
        if let Some(parent) = ir_out_path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(err) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create output directory {parent:?}: {err}");
                    std::process::exit(1);
                }
            }
        }
        match serde_json::to_string_pretty(&ir_bundle) {
            Ok(mut json) => {
                json.push('\n');
                if let Err(err) = fs::write(&ir_out_path, json) {
                    eprintln!("Failed to write IR JSON file {ir_out_path:?}: {err}");
                    std::process::exit(1);
                }
                eprintln!("ir_bundle: {}", ir_out_path.display());
            }
            Err(err) => {
                eprintln!("Failed to serialize IR as JSON: {err}");
                std::process::exit(1);
            }
        }
    }

    if !no_print_ir {
        match serde_json::to_string_pretty(&ir_bundle) {
            Ok(json) => println!("{json}"),
            Err(err) => {
                eprintln!("Failed to serialize IR as JSON: {err}");
                std::process::exit(1);
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedIrBundle {
    schema_version: u32,
    tool_version: String,
    cache_key: String,
    source_path: String,
    ir_bundle: IrBundle,
}

fn compile_pipeline_maybe_cached(
    loaded: &rust_plc::source_bundle::LoadedPlcSource,
    cache_dir: Option<&Path>,
) -> Result<IrBundle, Vec<String>> {
    let Some(cache_dir) = cache_dir else {
        return compile_pipeline(loaded);
    };

    let cache_key = compute_ir_cache_key(loaded)?;
    let cache_path = cache_dir.join(format!("{cache_key}.json"));
    if cache_path.is_file() {
        match read_cached_ir_bundle(&cache_path, &cache_key) {
            Ok(ir_bundle) => {
                eprintln!("ir_cache: hit {}", cache_path.display());
                return Ok(ir_bundle);
            }
            Err(err) => {
                eprintln!("ir_cache: ignored {} ({err})", cache_path.display());
            }
        }
    } else {
        eprintln!("ir_cache: miss {}", cache_path.display());
    }

    let reusable_verification = find_reusable_verification_cache(cache_dir, &cache_path);
    if let Some((path, _)) = &reusable_verification {
        eprintln!("verification_cache: candidate {}", path.display());
    }

    let (ir_bundle, reuse_report) = compile_pipeline_with_reusable_verification(
        loaded,
        reusable_verification.as_ref().map(|(_, reusable)| reusable),
    )?;
    if !reuse_report.reused_checkers.is_empty() {
        eprintln!(
            "verification_cache: reused [{}], checked [{}]",
            reuse_report.reused_checkers.join(","),
            reuse_report.checked_checkers.join(",")
        );
    }
    write_cached_ir_bundle(cache_dir, &cache_path, &cache_key, loaded, &ir_bundle)?;
    Ok(ir_bundle)
}

fn read_cached_ir_bundle(cache_path: &Path, expected_key: &str) -> Result<IrBundle, String> {
    let cached = read_cache_entry(cache_path)?;
    if cached.cache_key != expected_key {
        return Err("cache key mismatch".to_string());
    }
    Ok(cached.ir_bundle)
}

fn read_cache_entry(cache_path: &Path) -> Result<CachedIrBundle, String> {
    let text = fs::read_to_string(cache_path)
        .map_err(|err| format!("failed to read cache entry: {err}"))?;
    let cached: CachedIrBundle =
        serde_json::from_str(&text).map_err(|err| format!("failed to parse cache entry: {err}"))?;
    if cached.schema_version != IR_CACHE_SCHEMA_VERSION {
        return Err(format!(
            "schema_version {} != {}",
            cached.schema_version, IR_CACHE_SCHEMA_VERSION
        ));
    }
    if cached.tool_version != env!("CARGO_PKG_VERSION") {
        return Err(format!(
            "tool_version {} != {}",
            cached.tool_version,
            env!("CARGO_PKG_VERSION")
        ));
    }
    Ok(cached)
}

fn find_reusable_verification_cache(
    cache_dir: &Path,
    current_cache_path: &Path,
) -> Option<(PathBuf, rust_plc::verification::ReusableVerificationSummary)> {
    let entries = fs::read_dir(cache_dir).ok()?;
    let mut candidates = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path == current_cache_path {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        candidates.push((modified, path));
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0));

    for (_, path) in candidates {
        let Ok(cached) = read_cache_entry(&path) else {
            continue;
        };
        let Ok(reusable) = reusable_verification_from_ir_bundle(&cached.ir_bundle) else {
            continue;
        };
        return Some((path, reusable));
    }

    None
}

fn write_cached_ir_bundle(
    cache_dir: &Path,
    cache_path: &Path,
    cache_key: &str,
    loaded: &rust_plc::source_bundle::LoadedPlcSource,
    ir_bundle: &IrBundle,
) -> Result<(), Vec<String>> {
    fs::create_dir_all(cache_dir).map_err(|err| {
        vec![format!(
            "Failed to create IR cache directory {}: {err}",
            cache_dir.display()
        )]
    })?;
    let cached = CachedIrBundle {
        schema_version: IR_CACHE_SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        cache_key: cache_key.to_string(),
        source_path: loaded.requested_path.display().to_string(),
        ir_bundle: ir_bundle_ref_clone(ir_bundle)?,
    };
    let mut text = serde_json::to_string_pretty(&cached)
        .map_err(|err| vec![format!("Failed to serialize IR cache entry: {err}")])?;
    text.push('\n');
    fs::write(cache_path, text).map_err(|err| {
        vec![format!(
            "Failed to write IR cache entry {}: {err}",
            cache_path.display()
        )]
    })?;
    eprintln!("ir_cache: stored {}", cache_path.display());
    Ok(())
}

fn ir_bundle_ref_clone(ir_bundle: &IrBundle) -> Result<IrBundle, Vec<String>> {
    let value = serde_json::to_value(ir_bundle)
        .map_err(|err| vec![format!("Failed to serialize IR bundle for cache: {err}")])?;
    serde_json::from_value(value)
        .map_err(|err| vec![format!("Failed to deserialize IR bundle for cache: {err}")])
}

fn compute_ir_cache_key(
    loaded: &rust_plc::source_bundle::LoadedPlcSource,
) -> Result<String, Vec<String>> {
    let mut hasher = Sha256::new();
    hasher.update(b"rust_plc.ir_cache.v2\n");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\nsource_path\n");
    hasher.update(loaded.requested_path.to_string_lossy().as_bytes());
    hasher.update(b"\nsource\n");
    hasher.update(loaded.source.as_bytes());
    hasher.update(b"\ndependencies\n");
    for dependency in &loaded.dependencies {
        hasher.update(dependency.role.as_bytes());
        hasher.update(b"\0");
        hasher.update(dependency.path.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(dependency.sha256.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(b"\ndevices\n");
    update_hash_for_path_tree(&mut hasher, Path::new("devices"))?;
    Ok(hex::encode(hasher.finalize()))
}

fn update_hash_for_path_tree(hasher: &mut Sha256, root: &Path) -> Result<(), Vec<String>> {
    if !root.exists() {
        hasher.update(b"<missing>");
        return Ok(());
    }
    let mut files = Vec::new();
    collect_toml_files(root, &mut files)?;
    files.sort();
    for path in files {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        let bytes = fs::read(&path).map_err(|err| {
            vec![format!(
                "Failed to read cache input {}: {err}",
                path.display()
            )]
        })?;
        hasher.update(bytes);
        hasher.update(b"\0");
    }
    Ok(())
}

fn collect_toml_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Vec<String>> {
    let entries = fs::read_dir(dir).map_err(|err| {
        vec![format!(
            "Failed to read cache input dir {}: {err}",
            dir.display()
        )]
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            vec![format!(
                "Failed to read cache input entry in {}: {err}",
                dir.display()
            )]
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_toml_files(&path, out)?;
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn print_success_summary(summary: &VerificationSummary) {
    eprintln!("Verification passed:");
    eprintln!(
        "  - Safety: {} (depth {})",
        summary.safety.level, summary.safety.explored_depth
    );
    eprintln!(
        "    Coverage: bound {}/{}, degraded {}, skipped {}",
        summary.safety.coverage.bound_rules,
        summary.safety.coverage.total_rules,
        summary.safety.coverage.degraded_rules,
        summary.safety.coverage.skipped_rules
    );

    for warning in &summary.safety.warnings {
        eprintln!(
            "    [{}] {}",
            warning_level_label(&warning.level),
            warning.message
        );
    }

    eprintln!("  - Liveness: {}", summary.liveness.level);
    eprintln!("  - Timing: {}", summary.timing.level);
    eprintln!("  - Causality: {}", summary.causality.level);
}

fn warning_level_label(level: &WarningLevel) -> &'static str {
    match level {
        WarningLevel::Error => "ERROR",
        WarningLevel::Warn => "WARN",
        WarningLevel::Info => "INFO",
    }
}

fn collect_blocking_warnings(summary: &VerificationSummary) -> Vec<String> {
    let mut warnings = Vec::new();
    collect_checker_blocking_warnings("safety", &summary.safety.warnings, &mut warnings);
    collect_checker_blocking_warnings("liveness", &summary.liveness.warnings, &mut warnings);
    collect_checker_blocking_warnings("timing", &summary.timing.warnings, &mut warnings);
    collect_checker_blocking_warnings("causality", &summary.causality.warnings, &mut warnings);
    warnings
}

fn apply_runtime_budget_warnings(
    verification: &mut VerificationSummary,
    budget: &mut RuntimeBudget,
    thresholds: RuntimeBudgetThresholds,
) {
    let mut warnings: Vec<WarningEntry> = Vec::new();

    budget.recompute_time_estimate(
        thresholds.action_cost_us,
        thresholds.transition_cost_us,
        thresholds.parallel_expand_cost_us,
        thresholds.max_budget_time_estimate_us,
    );

    if budget.max_actions_per_transition > thresholds.max_actions_per_transition {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_actions_per_transition={} exceeds threshold {}",
                budget.max_actions_per_transition, thresholds.max_actions_per_transition
            ),
        });
    }
    if budget.max_actions_per_tick_upper_bound > thresholds.max_actions_per_tick_upper_bound {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_actions_per_tick_upper_bound={} exceeds threshold {}",
                budget.max_actions_per_tick_upper_bound,
                thresholds.max_actions_per_tick_upper_bound
            ),
        });
    }
    if budget.max_parallel_branches > thresholds.max_parallel_branches {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_parallel_branches={} exceeds threshold {}",
                budget.max_parallel_branches, thresholds.max_parallel_branches
            ),
        });
    }
    if budget.max_race_branches > thresholds.max_race_branches {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_race_branches={} exceeds threshold {}",
                budget.max_race_branches, thresholds.max_race_branches
            ),
        });
    }
    if thresholds.warn_on_same_tick_cycle && budget.has_same_tick_cycle {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: same-tick transition subgraph contains a cycle; runtime-core caps chaining to {} transitions per task per tick (active_tasks={})",
                budget.max_transitions_per_tick_cap, budget.active_task_count
            ),
        });
    }
    if budget.budget_time_estimate.exceeds_budget {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget time estimate: total_estimate_us={} exceeds threshold {}",
                budget.budget_time_estimate.total_estimate_us,
                budget.budget_time_estimate.max_allowed_us
            ),
        });
    }

    verification.timing.warnings.extend(warnings);
}
fn collect_checker_blocking_warnings(
    checker: &str,
    entries: &[WarningEntry],
    output: &mut Vec<String>,
) {
    for entry in entries {
        if matches!(entry.level, WarningLevel::Warn | WarningLevel::Error) {
            let code_suffix = entry
                .code
                .as_ref()
                .map(|code| format!(" ({code})"))
                .unwrap_or_default();
            output.push(format!(
                "[{checker}] {}{}: {}",
                warning_level_label(&entry.level),
                code_suffix,
                entry.message
            ));
        }
    }
}

fn default_verification_report_path(plc_path: &Path) -> PathBuf {
    let stem = plc_source_stem(plc_path);
    PathBuf::from("out").join(format!("{stem}.verification_report.json"))
}
