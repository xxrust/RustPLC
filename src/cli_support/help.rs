use std::fmt::Write as _;

pub(crate) struct CliCommandHelp {
    section: &'static str,
    name: &'static str,
    summary: &'static str,
    usage_template: &'static str,
}

const COMPILE_USAGE_TEMPLATE: &str = "Usage: {program} <source.plc|source.bundle.toml> [--report <verification_report.json>] [--deny-warnings] [--no-print-ir] [--ir-out <ir_bundle.json>] [--budget-... <value>]";

const CLI_COMMANDS: &[CliCommandHelp] = &[
    CliCommandHelp {
        section: "Core",
        name: "help",
        summary: "Show the top-level help screen or the usage for one command.",
        usage_template: "Usage: {program} help [command]",
    },
    CliCommandHelp {
        section: "Core",
        name: "new",
        summary: "Create a RustPLC project scaffold with starter PLC, scenarios, and VS Code tasks.",
        usage_template: "Usage: {program} new <project_dir> [--layout <single-file|structured-fragments>] [--delivery-layer <module|station|line>] [--force]",
    },
    CliCommandHelp {
        section: "Simulation",
        name: "sim",
        summary: "Run the built-in runtime-core demo program against a scenario YAML.",
        usage_template: "Usage: {program} sim <scenario.yaml> [--out <trace.jsonl>] [--vcd-out <wave.vcd>] [--analog-out <analog.csv>] [--report-out <report.json>]",
    },
    CliCommandHelp {
        section: "Simulation",
        name: "sim-plc",
        summary: "Compile a PLC file and execute it against a scenario with trace and audit outputs.",
        usage_template: "Usage: {program} sim-plc <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out <trace.jsonl> [--retain-config <retain.toml>] [--retain-state <retain_state.json>] [--enable-online-force-dev] [--online-force-script <script.jsonl>] [--online-force-audit-out <audit.jsonl>] [--online-var-script <script.jsonl>] [--online-var-bindings <bindings.toml>] [--online-var-audit-out <audit.jsonl>] [--alarm-audit-out <alarm_events.ndjson>] [--alarm-hmi-ws <ws://host:port/path>] [--alarm-scenario-id <id>] [--alarm-top <n>] [--alarm-dedup-window-ms <ms>] [--alarm-min-interval-ms <ms>] [--io-snapshot-out <io_snapshot.json>]",
    },
    CliCommandHelp {
        section: "Simulation",
        name: "sim-regress",
        summary: "Batch-run PLC and scenario directories and emit a regression summary.",
        usage_template: "Usage: {program} sim-regress --plc-dir <dir> --scenario-dir <dir> [--artifacts-dir <dir>] [--summary-out <summary.json>] [--minimize-failure]",
    },
    CliCommandHelp {
        section: "Simulation",
        name: "sim-pid-kpi",
        summary: "Run the PID KPI flow for a PLC file and a KPI scenario definition.",
        usage_template: "Usage: {program} sim-pid-kpi <source.plc|source.bundle.toml> --scenario <pid_scenario.yaml> [--out <kpi.json>]",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "build-rp2040",
        summary: "Build an RP2040 deployment bundle from a PLC file and optional I/O maps.",
        usage_template: "Usage: {program} build-rp2040 <source.plc|source.bundle.toml> --out <dir> [--io-map <file>] [--analog-calibration <file>] [--emit-uf2 <file.uf2>] [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "build-renode-stm32",
        summary: "Build a Renode-ready STM32F4 Discovery firmware ELF from a PLC file and scenario.",
        usage_template: "Usage: {program} build-renode-stm32 <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out <dir> [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "release-bundle",
        summary: "Assemble a no-board release bundle with scenario, build, and timing artifacts.",
        usage_template: "Usage: {program} release-bundle <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out-dir <dir> [--io-map <file>] [--max-p99-exec-us <us>] [--max-overrun-count <n>]",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "flash-rp2040",
        summary: "Copy a UF2 image to a mounted RP2040 board path.",
        usage_template: "Usage: {program} flash-rp2040 --uf2 <file.uf2> --mount <path> [--dry-run]",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "board-parse",
        summary: "Normalize board logs into structured artifacts for analysis.",
        usage_template: "Usage: {program} board-parse --in <board.log> --out-dir <dir>",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "no-board-gate",
        summary: "Run the no-board regression gate and emit release diagnostics.",
        usage_template: "Usage: {program} no-board-gate <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out-dir <dir> [--context <n>] [--sil-scenario <scenario.yaml>] [--board-scenario <scenario.yaml>] [--max-p99-exec-us <us>] [--max-overrun-count <n>] [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "project-check",
        summary: "Run the unified project regression check across compile, lint, doctor, and gate steps.",
        usage_template: "Usage: {program} project-check <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out-dir <dir> [--max-p99-exec-us <us>] [--max-overrun-count <n>] [--intent-contract <contract.json> --intent-evidence <trace.jsonl>] [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "commissioning-run",
        summary: "Generate a commissioning run bundle for a PLC file.",
        usage_template: "Usage: {program} commissioning-run <source.plc|source.bundle.toml> --out-dir <dir> [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "pil-run",
        summary: "Run a PLC file against a PIL scenario.",
        usage_template: "Usage: {program} pil-run <source.plc|source.bundle.toml> --scenario <scenario.yaml>",
    },
    CliCommandHelp {
        section: "Deployment",
        name: "virtual-board",
        summary: "Produce virtual-board artifacts from a PLC file and scenario.",
        usage_template: "Usage: {program} virtual-board <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out-dir <dir>",
    },
    CliCommandHelp {
        section: "Diagnostics",
        name: "geometry-export",
        summary: "Export a semantic-twin geometry artifact from topology, state-machine, constraints, and optional evidence.",
        usage_template: "Usage: {program} geometry-export <source.plc|source.bundle.toml> --out <geometry.json> [--trace <trace.jsonl>] [--intent-report <report.json>] [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Diagnostics",
        name: "trace-diff",
        summary: "Compare SIL and board traces and emit a mismatch report.",
        usage_template: "Usage: {program} trace-diff --sil <trace.jsonl> --board <trace.jsonl> --out <report.json> [--context <n>] [--fail-on-mismatch]",
    },
    CliCommandHelp {
        section: "Diagnostics",
        name: "trace-doctor",
        summary: "Correlate trace, diff, timing, and snapshot artifacts into diagnosis output.",
        usage_template: "Usage: {program} trace-doctor <source.plc|source.bundle.toml> --scenario <scenario.yaml> [--trace <trace.jsonl>] [--diff <diff_report.json>] [--timing-report <timing_report.json>] [--io-snapshot <io_snapshot.json>] [--evidence-source <no_board|hil_board|runtime_live|mixed>] [--top <n>] [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Diagnostics",
        name: "intent-doctor",
        summary: "Rank intent-anchor candidates from a real trace and diagnose binding or cycle-boundary instability.",
        usage_template: "Usage: {program} intent-doctor <source.plc|source.bundle.toml> --trace <trace.jsonl> [--intent-contract <contract.json>] [--top <n>] [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Diagnostics",
        name: "timing-report",
        summary: "Convert tick timing JSONL into a timing summary report.",
        usage_template: "Usage: {program} timing-report --in <tick_timing.jsonl> [--out <timing_report.json>]",
    },
    CliCommandHelp {
        section: "Diagnostics",
        name: "io-map-normalize",
        summary: "Normalize an IO map TOML file into the canonical form.",
        usage_template: "Usage: {program} io-map-normalize --in <io_map.toml> --out <normalized.toml>",
    },
    CliCommandHelp {
        section: "Components",
        name: "component-topology-validate",
        summary: "Validate a component topology JSON and optionally write a normalized copy.",
        usage_template: "Usage: {program} component-topology-validate <topology.json> [--output <human|json>] [--normalized-out <normalized_topology.json>]",
    },
    CliCommandHelp {
        section: "Components",
        name: "component-topology-diff",
        summary: "Diff two normalized component topology snapshots.",
        usage_template: "Usage: {program} component-topology-diff <before_topology.json> <after_topology.json> --out <semantic_diff.json> [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Components",
        name: "component-scenario-validate",
        summary: "Validate a component scenario JSON and optionally write a normalized copy.",
        usage_template: "Usage: {program} component-scenario-validate <scenario.json> [--output <human|json>] [--normalized-out <normalized_scenario.json>]",
    },
    CliCommandHelp {
        section: "Components",
        name: "component-sim",
        summary: "Simulate a component topology with optional trace, diagnosis, and audit outputs.",
        usage_template: "Usage: {program} component-sim <topology.json> --scenario <scenario.json> [--out <component_trace.jsonl>] [--fault-audit-out <fault_audit.jsonl>] [--diagnosis-out <component_diagnosis.json>] [--keypoints-out <component_keypoints.json>] [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Scenarios",
        name: "scenario-init",
        summary: "Generate a starter scenario YAML from a PLC file.",
        usage_template: "Usage: {program} scenario-init <source.plc|source.bundle.toml> [--out <scenario.yaml>] [--preset <minimal|normal|timeout|sensor_stuck|bounce>]",
    },
    CliCommandHelp {
        section: "Scenarios",
        name: "scenario-validate",
        summary: "Validate one scenario YAML against a PLC file.",
        usage_template: "Usage: {program} scenario-validate <source.plc|source.bundle.toml> --scenario <scenario.yaml> [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Scenarios",
        name: "scenario-doctor",
        summary: "Diagnose one PLC and scenario pair and optionally preview fixes.",
        usage_template: "Usage: {program} scenario-doctor <source.plc|source.bundle.toml> --scenario <scenario.yaml> [--fix-preview] [--output <human|json>]",
    },
    CliCommandHelp {
        section: "Scenarios",
        name: "scenario-expand",
        summary: "Expand one scenario YAML into the resolved form used by simulation.",
        usage_template: "Usage: {program} scenario-expand <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out <expanded.yaml>",
    },
    CliCommandHelp {
        section: "Scenarios",
        name: "scenario-gen",
        summary: "Generate scenario suites from a PLC file and a generation config.",
        usage_template: "Usage: {program} scenario-gen --plc <source.plc|source.bundle.toml> --config <gen.yaml> --out-dir <dir> [--coverage-mode <pairwise|boundary-first|risk-first>] [--dry-run] [--template-library <metadata.json>]",
    },
    CliCommandHelp {
        section: "Utilities",
        name: "sequence-lint",
        summary: "Lint critical wait recovery patterns in a PLC program.",
        usage_template: "Usage: {program} sequence-lint <source.plc|source.bundle.toml> [--critical-wait-level <warn|error>] [--critical-wait-exempt <task.step|task.*>]",
    },
    CliCommandHelp {
        section: "Utilities",
        name: "gen-st",
        summary: "Generate IEC 61131-3 ST output from a PLC file.",
        usage_template: "Usage: {program} gen-st <source.plc|source.bundle.toml> [--out <output.st>] [--program-name <Main>] [--task-interval-ms <ms>] [--no-verification-summary]",
    },
];

pub(crate) fn is_help_flag(arg: &str) -> bool {
    matches!(arg, "-h" | "--help")
}

pub(crate) fn cli_command_help(name: &str) -> Option<&'static CliCommandHelp> {
    CLI_COMMANDS.iter().find(|command| command.name == name)
}

pub(crate) fn command_usage(program: &str, command: &str) -> String {
    cli_command_help(command)
        .map(|entry| entry.usage_template.replace("{program}", program))
        .unwrap_or_else(|| format!("Usage: {program} {command}"))
}

fn command_help_options(command: &str) -> &'static [&'static str] {
    match command {
        "help" => &["[command]                  Show detailed help for one command."],
        "new" => &[
            "--layout <single-file|structured-fragments> Choose the scaffold source layout.",
            "--delivery-layer <module|station|line> Choose the default delivery asset layer for scaffold docs and entries.",
            "--force                    Overwrite known scaffold files in a non-empty directory.",
        ],
        "sim" => &[
            "--out <trace.jsonl>          Write runtime trace JSONL.",
            "--vcd-out <wave.vcd>         Write digital waveform VCD output.",
            "--analog-out <analog.csv>    Write analog output samples as CSV.",
            "--report-out <report.json>   Write the simulation summary JSON.",
        ],
        "sim-plc" => &[
            "--scenario <scenario.yaml>   Scenario YAML to execute.",
            "--out <trace.jsonl>          Required runtime trace output.",
            "--retain-config <retain.toml> Load and persist retain channels.",
            "--retain-state <state.json>  Override the retain state path.",
            "--enable-online-force-dev    Enable dev-only online force and variable controls.",
            "--online-force-script <script.jsonl> Inject online force commands.",
            "--online-var-script <script.jsonl> Inject online variable commands.",
            "--online-var-bindings <bindings.toml> Map script keys to runtime channels.",
            "--alarm-audit-out <events.ndjson>    Write runtime alarm events.",
            "--alarm-hmi-ws <ws://...>    Mirror alarm events to a websocket endpoint.",
            "--io-snapshot-out <io_snapshot.json> Capture per-tick IO snapshots.",
        ],
        "sim-regress" => &[
            "--plc-dir <dir>              Directory of PLC inputs.",
            "--scenario-dir <dir>         Directory of scenario YAMLs.",
            "--artifacts-dir <dir>        Output directory for traces and failures.",
            "--summary-out <summary.json> Write the regression summary JSON.",
            "--minimize-failure           Attempt scenario minimization for failures.",
        ],
        "sim-pid-kpi" => &[
            "--scenario <pid_scenario.yaml> PID plant and KPI configuration.",
            "--out <kpi.json>             Destination KPI report JSON.",
        ],
        "build-rp2040" => &[
            "--out <dir>                  Required build output directory.",
            "--io-map <file>              Validate and embed an explicit IO map.",
            "--analog-calibration <file>  Apply analog scaling/offset calibration.",
            "--emit-uf2 <file.uf2>        Produce a flashable UF2 image.",
            "--output <human|json>        Select CLI output format.",
        ],
        "build-renode-stm32" => &[
            "--scenario <scenario.yaml>   Required scenario compiled into the ELF.",
            "--out <dir>                  Required build output directory.",
            "--output <human|json>        Select CLI output format.",
        ],
        "release-bundle" => &[
            "--scenario <scenario.yaml>   Scenario used for gate and bundle artifacts.",
            "--out-dir <dir>              Required release bundle directory.",
            "--io-map <file>              Include an explicit IO map in the bundle.",
            "--max-p99-exec-us <us>       Realtime threshold for p99 execution.",
            "--max-overrun-count <n>      Realtime threshold for overruns.",
        ],
        "flash-rp2040" => &[
            "--uf2 <file.uf2>             UF2 image to copy.",
            "--mount <path>               Mounted RP2040 mass-storage path.",
            "--dry-run                    Print the planned copy without writing.",
        ],
        "board-parse" => &[
            "--in <board.log>             Input board log file.",
            "--out-dir <dir>              Output directory for normalized artifacts.",
        ],
        "no-board-gate" => &[
            "--scenario <scenario.yaml>   Shared scenario for both SIL and virtual-board legs.",
            "--sil-scenario <scenario.yaml> Override the SIL scenario.",
            "--board-scenario <scenario.yaml> Override the virtual-board scenario.",
            "--out-dir <dir>              Required gate artifact directory.",
            "--context <n>                Mismatch context window for trace diff.",
            "--max-p99-exec-us <us>       Realtime threshold for p99 execution.",
            "--max-overrun-count <n>      Realtime threshold for overruns.",
            "--output <human|json>        Select CLI output format.",
        ],
        "project-check" => &[
            "--scenario <scenario.yaml>   Scenario shared by the doctor and no-board gate steps.",
            "--out-dir <dir>              Required output directory for per-step artifacts.",
            "--max-p99-exec-us <us>       Realtime threshold forwarded to no-board-gate.",
            "--max-overrun-count <n>      Overrun threshold forwarded to no-board-gate.",
            "--intent-contract <file>     Optional intent-alignment contract fixture for an extra project-check step.",
            "--intent-evidence <file>     Optional observed trace JSONL paired with --intent-contract.",
            "--output <human|json>        Select CLI output format.",
        ],
        "commissioning-run" => &[
            "--out-dir <dir>              Required commissioning artifact directory.",
            "--output <human|json>        Select CLI output format.",
        ],
        "pil-run" => &["--scenario <scenario.yaml>   Scenario YAML to replay against the runtime."],
        "virtual-board" => &[
            "--scenario <scenario.yaml>   Scenario YAML to replay.",
            "--out-dir <dir>              Required output directory for board-like artifacts.",
        ],
        "geometry-export" => &[
            "--out <geometry.json>        Required semantic-twin geometry artifact output.",
            "--trace <trace.jsonl>        Optional runtime trace overlay input.",
            "--intent-report <report.json> Optional intent-alignment report overlay input.",
            "--output <human|json>        Select CLI output format.",
        ],
        "trace-diff" => &[
            "--sil <trace.jsonl>          SIL trace JSONL input.",
            "--board <trace.jsonl>        Board or virtual-board trace JSONL input.",
            "--out <report.json>          Required diff report JSON.",
            "--context <n>                Context window around mismatches.",
            "--fail-on-mismatch           Exit non-zero when traces diverge.",
        ],
        "trace-doctor" => &[
            "--scenario <scenario.yaml>   Scenario used to interpret artifacts.",
            "--trace <trace.jsonl>        Optional runtime trace input.",
            "--diff <diff_report.json>    Optional trace diff input.",
            "--timing-report <report.json> Optional timing report input.",
            "--io-snapshot <snapshot.json> Optional IO snapshot input.",
            "--evidence-source <...>      Label the artifact origin in diagnosis output.",
            "--top <n>                    Limit the human report to the top N candidates.",
            "--output <human|json>        Select CLI output format.",
        ],
        "intent-doctor" => &[
            "--trace <trace.jsonl>        Required runtime trace input.",
            "--intent-contract <file>     Optional contract fixture to diagnose existing milestone bindings.",
            "--top <n>                    Limit the human report to the top N anchor candidates.",
            "--output <human|json>        Select CLI output format.",
        ],
        "timing-report" => &[
            "--in <tick_timing.jsonl>     Required timing JSONL input.",
            "--out <timing_report.json>   Override the output report path.",
        ],
        "io-map-normalize" => &[
            "--in <io_map.toml>           Required source IO map.",
            "--out <normalized.toml>      Required normalized output path.",
        ],
        "component-topology-validate" => &[
            "--output <human|json>        Select CLI output format.",
            "--normalized-out <file>      Write the normalized topology JSON.",
        ],
        "component-topology-diff" => &[
            "--out <semantic_diff.json>   Required semantic diff output path.",
            "--output <human|json>        Select CLI output format.",
        ],
        "component-scenario-validate" => &[
            "--output <human|json>        Select CLI output format.",
            "--normalized-out <file>      Write the normalized scenario JSON.",
        ],
        "component-sim" => &[
            "--scenario <scenario.json>   Required component scenario input.",
            "--out <component_trace.jsonl> Write per-tick trace JSONL.",
            "--fault-audit-out <fault_audit.jsonl> Write fault audit JSONL.",
            "--diagnosis-out <component_diagnosis.json> Write diagnosis JSON.",
            "--keypoints-out <component_keypoints.json> Write keypoint artifact JSON.",
            "--output <human|json>        Select CLI output format.",
        ],
        "scenario-init" => &[
            "--out <scenario.yaml>        Override the generated scenario path.",
            "--preset <minimal|normal|timeout|sensor_stuck|bounce> Choose the template preset.",
        ],
        "scenario-validate" => &[
            "--scenario <scenario.yaml>   Required scenario YAML to validate.",
            "--output <human|json>        Select CLI output format.",
        ],
        "scenario-doctor" => &[
            "--scenario <scenario.yaml>   Required scenario YAML to diagnose.",
            "--fix-preview                Include suggested fixes in the report.",
            "--output <human|json>        Select CLI output format.",
        ],
        "scenario-expand" => &[
            "--scenario <scenario.yaml>   Required source scenario YAML.",
            "--out <expanded.yaml>        Required resolved scenario output.",
        ],
        "scenario-gen" => &[
            "--plc <source.plc|source.bundle.toml> Required PLC input.",
            "--config <gen.yaml>          Required scenario generation config.",
            "--out-dir <dir>              Required output directory.",
            "--coverage-mode <pairwise|boundary-first|risk-first> Scenario selection strategy.",
            "--dry-run                    Print the plan without writing files.",
            "--template-library <metadata.json> Optional template metadata input.",
        ],
        "sequence-lint" => &[
            "--critical-wait-level <warn|error> Severity for critical wait findings.",
            "--critical-wait-exempt <task.step|task.*> Exempt a task or step pattern.",
        ],
        "gen-st" => &[
            "--out <output.st>            Write ST output to a file instead of stdout.",
            "--program-name <Main>        Override the emitted ST program name.",
            "--task-interval-ms <ms>      Override the OpenPLC cyclic task interval in ms.",
            "--no-verification-summary    Omit verification comments from ST output.",
        ],
        _ => &[],
    }
}

fn command_help_notes(command: &str) -> &'static [&'static str] {
    match command {
        "help" => {
            &["`help compile` shows the detailed page for the default compile-and-verify mode."]
        }
        "new" => &[
            "`single-file` keeps the existing Day-1 scaffold shape.",
            "`structured-fragments` creates a phased directory layout (00_topology/ through 07_hmi/) with a v2 bundle entry for multi-agent projects.",
            "`--delivery-layer` defaults to `station` and is recorded as metadata in rustplc.project.toml.",
        ],
        "sim" => &["This command runs the built-in demo program, not a user PLC file."],
        "sim-plc" => &[
            "Online force and online variable controls stay disabled unless `--enable-online-force-dev` is present.",
        ],
        "build-rp2040" => {
            &["`--emit-uf2` requires `--io-map` so the board pin contract is explicit."]
        }
        "build-renode-stm32" => &[
            "This command embeds both the generated runtime program and the resolved scenario into a local STM32F4 Discovery firmware image for Renode.",
        ],
        "release-bundle" => &[
            "The bundle reuses compile, simulation, timing, and gate artifacts instead of inventing a parallel flow.",
        ],
        "flash-rp2040" => &["The target mount path must already exist and be writable."],
        "no-board-gate" => &[
            "If `--sil-scenario` or `--board-scenario` is omitted, the shared `--scenario` path is reused.",
        ],
        "project-check" => &[
            "This command orchestrates `compile`, `sequence-lint`, `scenario-doctor`, and `no-board-gate` as one reproducible release check.",
            "When `--intent-contract` and `--intent-evidence` are both provided, an `intent_alignment` step is appended and reduced from the library report without reinterpreting its verdict.",
        ],
        "geometry-export" => &[
            "This command writes a stable JSON artifact for later SVG, web, or animation rendering; it does not render UI directly.",
            "When `--trace` is provided, runtime task and step names are resolved with a best-effort mapping from semantic task contexts.",
        ],
        "trace-doctor" => &["At least one of `--trace` or `--diff` is required."],
        "intent-doctor" => &[
            "If `--intent-contract` is omitted, the command tries the sibling `*.intent_alignment.contract.json` path next to the PLC source entry.",
            "Use this before finalizing milestone bindings for a new project so anchor selection is based on real trace evidence, not guessed `task.step` names.",
        ],
        "component-sim" => {
            &["Topology and scenario inputs are validated before simulation starts."]
        }
        "scenario-init" => {
            &["The default output path is derived from the PLC path when `--out` is omitted."]
        }
        "scenario-expand" => &[
            "Expansion resolves device-name aliases and writes the canonical scenario form used by runtime tools.",
        ],
        "scenario-gen" => &[
            "Generated scenario cases are selected from the config according to the chosen coverage mode.",
        ],
        "gen-st" => &[
            "`gen-st` compiles the PLC first, so semantic or verification failures still stop the command.",
        ],
        _ => &[],
    }
}

fn command_help_examples(command: &str) -> &'static [&'static str] {
    match command {
        "help" => &["rust_plc help sim-plc", "rust_plc help compile"],
        "new" => &[
            "rust_plc new demo_project",
            "rust_plc new wafer_loader --layout structured-fragments",
            "rust_plc new pick_head --layout structured-fragments --delivery-layer module",
            "rust_plc new demo_project --force",
        ],
        "sim" => &["rust_plc sim scenarios/basic.yaml --out out/sim/trace.jsonl"],
        "sim-plc" => &[
            "rust_plc sim-plc examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --out out/sim/trace.jsonl",
        ],
        "sim-regress" => &[
            "rust_plc sim-regress --plc-dir examples --scenario-dir scenarios --summary-out out/sim-regress/summary.json",
        ],
        "sim-pid-kpi" => &[
            "rust_plc sim-pid-kpi <source.plc|source.bundle.toml> --scenario <pid_scenario.yaml> --out out/pid/kpi.json",
        ],
        "build-rp2040" => &[
            "rust_plc build-rp2040 examples/rp2040_motion_minimal.plc --out out/rp2040 --io-map examples/rp2040_motion_minimal.io_map.toml",
        ],
        "build-renode-stm32" => &[
            "rust_plc build-renode-stm32 examples/pil_baselines/case_timeout/case.plc --scenario examples/pil_baselines/case_timeout/scenarios/base.yaml --out out/renode_case_timeout",
        ],
        "release-bundle" => &[
            "rust_plc release-bundle examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --out-dir out/release/rp2040_motion_minimal",
        ],
        "flash-rp2040" => &["rust_plc flash-rp2040 --uf2 out/rp2040/app.uf2 --mount E:\\"],
        "board-parse" => &["rust_plc board-parse --in board.log --out-dir out/board_parse"],
        "no-board-gate" => &[
            "rust_plc no-board-gate examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --out-dir out/gate/no_board/rp2040_motion_minimal --output json",
        ],
        "project-check" => &[
            "rust_plc project-check examples/project_scaffold_demo/plc/main.plc --scenario examples/project_scaffold_demo/scenarios/nominal/normal.yaml --out-dir out/project_check/project_scaffold_demo --output human",
        ],
        "commissioning-run" => &[
            "rust_plc commissioning-run examples/project_scaffold_demo/plc/main.plc --out-dir out/commissioning/project_scaffold_demo",
        ],
        "pil-run" => &[
            "rust_plc pil-run examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml",
        ],
        "virtual-board" => &[
            "rust_plc virtual-board examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --out-dir out/virtual_board/rp2040_motion_minimal",
        ],
        "geometry-export" => &[
            "rust_plc geometry-export examples/rp2040_motion_minimal.plc --out out/geometry/rp2040_motion_minimal.geometry.json",
            "rust_plc geometry-export out/wafer_loader_project/plc/main.target_semantics.bundle.toml --trace out/wafer_loader_project/out/project_check_with_auto_sim_v4/no_board_gate/artifacts/sil_trace.jsonl --intent-report out/wafer_loader_project/out/project_check_with_auto_sim_v4/intent_alignment/report.json --out out/geometry/wafer_loader.geometry.json --output json",
        ],
        "trace-diff" => &[
            "rust_plc trace-diff --sil out/sil_trace.jsonl --board out/board_trace.jsonl --out out/diff_report.json --fail-on-mismatch",
        ],
        "trace-doctor" => &[
            "rust_plc trace-doctor examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --trace out/sim/trace.jsonl --diff out/diff_report.json --output human",
        ],
        "intent-doctor" => &[
            "rust_plc intent-doctor out/wafer_loader_project/plc/main.target_semantics.bundle.toml --trace out/wafer_loader_project/out/project_check_with_auto_sim_v4/no_board_gate/artifacts/sil_trace.jsonl --output human",
        ],
        "timing-report" => {
            &["rust_plc timing-report --in out/tick_timing.jsonl --out out/timing_report.json"]
        }
        "io-map-normalize" => {
            &["rust_plc io-map-normalize --in config/io_map.toml --out out/io_map.normalized.toml"]
        }
        "component-topology-validate" => &[
            "rust_plc component-topology-validate examples/component_model/topology.json --normalized-out out/topology.normalized.json",
        ],
        "component-topology-diff" => &[
            "rust_plc component-topology-diff before.json after.json --out out/topology.diff.json",
        ],
        "component-scenario-validate" => &[
            "rust_plc component-scenario-validate examples/component_model/scenario_normal.json --output json",
        ],
        "component-sim" => &[
            "rust_plc component-sim examples/component_model/topology.json --scenario examples/component_model/scenario_normal.json --out out/component_trace.jsonl",
        ],
        "scenario-init" => &[
            "rust_plc scenario-init examples/project_scaffold_demo/plc/main.plc --preset normal --out scenarios/generated/project_scaffold_demo.normal.yaml",
        ],
        "scenario-validate" => &[
            "rust_plc scenario-validate examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --output json",
        ],
        "scenario-doctor" => &[
            "rust_plc scenario-doctor examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --fix-preview --output human",
        ],
        "scenario-expand" => &[
            "rust_plc scenario-expand examples/project_scaffold_demo/plc/main.plc --scenario examples/scenarios/pulse_hold.yaml --out out/scenario.expanded.yaml",
        ],
        "scenario-gen" => &[
            "rust_plc scenario-gen --plc examples/rp2040_motion_minimal.plc --config examples/scenario_gen/basic.yaml --out-dir out/scenario_gen",
        ],
        "sequence-lint" => &[
            "rust_plc sequence-lint examples/recovery_templates/power_loss_recovery.plc --critical-wait-level error",
        ],
        "gen-st" => &[
            "rust_plc gen-st examples/dual_axis_platform.plc --out out/codegen/dual_axis_platform.st",
        ],
        _ => &[],
    }
}

fn write_help_section(msg: &mut String, title: &str, lines: &[&str]) {
    if lines.is_empty() {
        return;
    }
    writeln!(msg).expect("write help section");
    writeln!(msg, "{title}:").expect("write help section");
    for line in lines {
        writeln!(msg, "  {line}").expect("write help section");
    }
}

fn render_compile_help(program: &str) -> String {
    let mut msg = String::new();
    writeln!(
        &mut msg,
        "{}",
        COMPILE_USAGE_TEMPLATE.replace("{program}", program)
    )
    .expect("write compile help");
    writeln!(&mut msg).expect("write compile help");
    writeln!(
        &mut msg,
        "Parse, verify, and emit IR JSON for a PLC file when no explicit subcommand is given."
    )
    .expect("write compile help");
    writeln!(&mut msg).expect("write compile help");
    writeln!(&mut msg, "Core options:").expect("write compile help");
    writeln!(
        &mut msg,
        "  --report <verification_report.json>  Write the verification report JSON."
    )
    .expect("write compile help");
    writeln!(
        &mut msg,
        "  --deny-warnings                      Exit non-zero when blocking warnings are present."
    )
    .expect("write compile help");
    writeln!(
        &mut msg,
        "  --no-print-ir                        Suppress the default IR JSON stdout output."
    )
    .expect("write compile help");
    writeln!(
        &mut msg,
        "  --ir-out <ir_bundle.json>            Write the IR bundle JSON to a file."
    )
    .expect("write compile help");
    writeln!(&mut msg).expect("write compile help");
    writeln!(&mut msg, "Budget options (also configurable via env vars):")
        .expect("write compile help");
    writeln!(&mut msg, "  --budget-max-actions-per-transition <n>").expect("write compile help");
    writeln!(&mut msg, "  --budget-max-actions-per-tick <n>").expect("write compile help");
    writeln!(&mut msg, "  --budget-max-parallel-branches <n>").expect("write compile help");
    writeln!(&mut msg, "  --budget-max-race-branches <n>").expect("write compile help");
    writeln!(&mut msg, "  --budget-warn-on-same-tick-cycle <true|false>")
        .expect("write compile help");
    writeln!(&mut msg, "  --budget-action-cost-us <n>").expect("write compile help");
    writeln!(&mut msg, "  --budget-transition-cost-us <n>").expect("write compile help");
    writeln!(&mut msg, "  --budget-parallel-expand-cost-us <n>").expect("write compile help");
    writeln!(&mut msg, "  --budget-max-time-estimate-us <n>").expect("write compile help");
    write_help_section(
        &mut msg,
        "Examples",
        &[
            "rust_plc examples/dual_axis_platform.plc --report out/verification_report.json",
            "rust_plc examples/project_scaffold_demo/plc/main.plc --ir-out out/ir_bundle.json --no-print-ir",
        ],
    );
    msg.trim_end().to_string()
}

fn render_command_help(program: &str, command: &str) -> Option<String> {
    if command == "compile" {
        return Some(render_compile_help(program));
    }

    let entry = cli_command_help(command)?;
    let mut msg = String::new();
    writeln!(
        &mut msg,
        "{}",
        entry.usage_template.replace("{program}", program)
    )
    .expect("write command help");
    writeln!(&mut msg).expect("write command help");
    writeln!(&mut msg, "{}", entry.summary).expect("write command help");
    write_help_section(&mut msg, "Options", command_help_options(command));
    write_help_section(&mut msg, "Notes", command_help_notes(command));
    write_help_section(&mut msg, "Examples", command_help_examples(command));
    writeln!(&mut msg).expect("write command help");
    writeln!(&mut msg, "Run `{program} help` to list all commands.").expect("write command help");
    Some(msg.trim_end().to_string())
}

fn render_root_help(program: &str) -> String {
    let mut msg = String::new();
    writeln!(&mut msg, "Usage:").expect("write root help");
    writeln!(
        &mut msg,
        "  {}",
        COMPILE_USAGE_TEMPLATE
            .replace("{program}", program)
            .trim_start_matches("Usage: ")
    )
    .expect("write root help");
    writeln!(&mut msg, "  {program} <command> [options]").expect("write root help");
    writeln!(&mut msg, "  {program} help [command]").expect("write root help");
    writeln!(&mut msg).expect("write root help");
    writeln!(
        &mut msg,
        "Default action: compile and verify a PLC file when no subcommand is given."
    )
    .expect("write root help");
    writeln!(&mut msg).expect("write root help");
    writeln!(&mut msg, "Commands:").expect("write root help");

    let mut current_section = "";
    for entry in CLI_COMMANDS {
        if entry.section != current_section {
            current_section = entry.section;
            writeln!(&mut msg, "  {current_section}:").expect("write root help");
        }
        writeln!(&mut msg, "    {:<28} {}", entry.name, entry.summary).expect("write root help");
    }

    writeln!(&mut msg).expect("write root help");
    writeln!(
        &mut msg,
        "Use `{program} help <command>` or `{program} <command> --help` for command details."
    )
    .expect("write root help");
    msg.trim_end().to_string()
}

pub(crate) fn help_requested_for_invocation(_first: &str, remaining: &[String]) -> bool {
    remaining.iter().any(|arg| is_help_flag(arg))
}

pub(crate) fn print_command_help_and_exit(program: &str, command: &str, exit_code: i32) -> ! {
    match render_command_help(program, command) {
        Some(help) => {
            eprintln!("{help}");
            std::process::exit(exit_code);
        }
        None => {
            eprintln!("Unknown command: {command}");
            eprintln!();
            print_usage(program);
            std::process::exit(1);
        }
    }
}

pub(crate) fn print_usage(program: &str) {
    eprintln!("{}", render_root_help(program));
}
