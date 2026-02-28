# Extern Call Performance Gate

This benchmark gate tracks extern-call overhead for two paths:

- `simple_add`: lightweight built-in `add(a, b)` call overhead
- `complex_quadratic_fit`: heavier built-in `quadratic_fit(...)` call overhead

## Benchmark Runner

- Binary: `src/bin/extern_perf_bench.rs`
- Output metric: microseconds per call (`us/call`) with mean/min/max and p95

Run manually:

```bash
cargo run --release --bin extern_perf_bench -- --output human
```

JSON output:

```bash
cargo run --release --bin extern_perf_bench -- --output json
```

## Regression Gate

- Script: `scripts/extern_perf_gate.py`
- Absolute thresholds: `scripts/perf/extern_perf_thresholds.json`
- Baseline reference: `scripts/perf/extern_perf_baseline.json`

Run gate locally:

```bash
python3 scripts/extern_perf_gate.py --output human
```

Automation-friendly JSON output:

```bash
python3 scripts/extern_perf_gate.py --output json
```

Gate fails when either condition is hit:

1. Measured p95 exceeds absolute threshold.
2. Measured p95 exceeds baseline by `max_regression_pct_vs_baseline`.
