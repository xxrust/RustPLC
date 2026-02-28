use rust_plc::extern_functions::ExternFunctionRegistry;
use serde::Serialize;
use std::env;
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug)]
struct Config {
    samples: usize,
    warmups: usize,
    simple_iterations: usize,
    complex_iterations: usize,
    output: OutputFormat,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            samples: 7,
            warmups: 2,
            simple_iterations: 100_000,
            complex_iterations: 20_000,
            output: OutputFormat::Human,
        }
    }
}

#[derive(Debug, Serialize)]
struct Metric {
    samples_us: Vec<f64>,
    mean_us: f64,
    p95_us: f64,
    min_us: f64,
    max_us: f64,
}

#[derive(Debug, Serialize)]
struct BenchPayload {
    schema_version: u32,
    samples: usize,
    warmups: usize,
    simple_iterations: usize,
    complex_iterations: usize,
    metrics_us_per_call: MetricSet,
}

#[derive(Debug, Serialize)]
struct MetricSet {
    simple_add: Metric,
    complex_quadratic_fit: Metric,
}

fn parse_args() -> Result<Config, String> {
    let mut cfg = Config::default();
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--samples" => {
                cfg.samples = parse_usize_arg(args.next(), "--samples")?;
            }
            "--warmups" => {
                cfg.warmups = parse_usize_arg(args.next(), "--warmups")?;
            }
            "--simple-iterations" => {
                cfg.simple_iterations = parse_usize_arg(args.next(), "--simple-iterations")?;
            }
            "--complex-iterations" => {
                cfg.complex_iterations = parse_usize_arg(args.next(), "--complex-iterations")?;
            }
            "--output" => {
                let Some(value) = args.next() else {
                    return Err("missing value for --output".to_string());
                };
                cfg.output = match value.as_str() {
                    "human" => OutputFormat::Human,
                    "json" => OutputFormat::Json,
                    _ => {
                        return Err(format!(
                            "invalid --output value `{value}` (expected human|json)"
                        ));
                    }
                };
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                return Err(format!("unknown argument `{arg}`"));
            }
        }
    }

    if cfg.samples == 0 {
        return Err("--samples must be greater than 0".to_string());
    }
    if cfg.simple_iterations == 0 {
        return Err("--simple-iterations must be greater than 0".to_string());
    }
    if cfg.complex_iterations == 0 {
        return Err("--complex-iterations must be greater than 0".to_string());
    }

    Ok(cfg)
}

fn parse_usize_arg(value: Option<String>, name: &str) -> Result<usize, String> {
    let Some(raw) = value else {
        return Err(format!("missing value for {name}"));
    };
    raw.parse::<usize>()
        .map_err(|_| format!("invalid integer for {name}: `{raw}`"))
}

fn print_help() {
    println!(
        "extern_perf_bench\n\
         Measures extern call overhead for simple (add) and complex (quadratic_fit) built-ins.\n\n\
         Usage:\n\
           cargo run --release --bin extern_perf_bench -- [options]\n\n\
         Options:\n\
           --samples <N>              Measured samples per metric (default: 7)\n\
           --warmups <N>              Warmup runs per metric (default: 2)\n\
           --simple-iterations <N>    Calls per sample for add() (default: 100000)\n\
           --complex-iterations <N>   Calls per sample for quadratic_fit() (default: 20000)\n\
           --output <human|json>      Output format (default: human)"
    );
}

fn percentile_p95(values: &[f64]) -> f64 {
    let mut ordered = values.to_vec();
    ordered.sort_by(|a, b| a.total_cmp(b));
    let index = ((ordered.len() - 1) as f64 * 0.95).round() as usize;
    ordered[index]
}

fn rounded(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn measure_us_per_call<F>(samples: usize, warmups: usize, iterations: usize, mut op: F) -> Metric
where
    F: FnMut(),
{
    let mut sample_values = Vec::with_capacity(samples);

    for run in 0..(samples + warmups) {
        let started = Instant::now();
        for _ in 0..iterations {
            op();
        }
        let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0;
        let per_call = elapsed_us / iterations as f64;
        if run >= warmups {
            sample_values.push(per_call);
        }
    }

    let sum: f64 = sample_values.iter().sum();
    let mean = sum / sample_values.len() as f64;
    let p95 = percentile_p95(&sample_values);
    let min = sample_values
        .iter()
        .copied()
        .min_by(|a, b| a.total_cmp(b))
        .unwrap_or(0.0);
    let max = sample_values
        .iter()
        .copied()
        .max_by(|a, b| a.total_cmp(b))
        .unwrap_or(0.0);

    Metric {
        samples_us: sample_values.into_iter().map(rounded).collect(),
        mean_us: rounded(mean),
        p95_us: rounded(p95),
        min_us: rounded(min),
        max_us: rounded(max),
    }
}

fn build_payload(cfg: &Config) -> BenchPayload {
    let registry = ExternFunctionRegistry::new();
    let simple_args = [1.25_f32, 2.5_f32];
    let complex_args = [-2.0_f32, -1.0, 0.0, 1.0, 2.0, 9.0, 2.0, 1.0, 6.0, 17.0];

    let simple_add = measure_us_per_call(cfg.samples, cfg.warmups, cfg.simple_iterations, || {
        let result = registry
            .call("add", &simple_args)
            .expect("add extern benchmark call should succeed");
        black_box(result);
    });

    let complex_quadratic_fit =
        measure_us_per_call(cfg.samples, cfg.warmups, cfg.complex_iterations, || {
            let result = registry
                .call("quadratic_fit", &complex_args)
                .expect("quadratic_fit extern benchmark call should succeed");
            black_box(result);
        });

    BenchPayload {
        schema_version: 1,
        samples: cfg.samples,
        warmups: cfg.warmups,
        simple_iterations: cfg.simple_iterations,
        complex_iterations: cfg.complex_iterations,
        metrics_us_per_call: MetricSet {
            simple_add,
            complex_quadratic_fit,
        },
    }
}

fn run() -> Result<(), String> {
    let cfg = parse_args()?;
    let payload = build_payload(&cfg);

    match cfg.output {
        OutputFormat::Human => {
            println!(
                "extern-perf-bench: samples={} warmups={} simple_iters={} complex_iters={}",
                cfg.samples, cfg.warmups, cfg.simple_iterations, cfg.complex_iterations
            );
            println!(
                "- simple_add: mean={:.3}us p95={:.3}us min={:.3}us max={:.3}us",
                payload.metrics_us_per_call.simple_add.mean_us,
                payload.metrics_us_per_call.simple_add.p95_us,
                payload.metrics_us_per_call.simple_add.min_us,
                payload.metrics_us_per_call.simple_add.max_us
            );
            println!(
                "- complex_quadratic_fit: mean={:.3}us p95={:.3}us min={:.3}us max={:.3}us",
                payload.metrics_us_per_call.complex_quadratic_fit.mean_us,
                payload.metrics_us_per_call.complex_quadratic_fit.p95_us,
                payload.metrics_us_per_call.complex_quadratic_fit.min_us,
                payload.metrics_us_per_call.complex_quadratic_fit.max_us
            );
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload)
                    .map_err(|err| format!("failed to serialize benchmark payload: {err}"))?
            );
        }
    }

    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("extern-perf-bench: ERROR: {err}");
        std::process::exit(1);
    }
}
