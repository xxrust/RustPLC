# PLC Optimization Pipeline

Date: 2026-03-25

This page documents the first optimization pipeline implemented for RustPLC.

The key constraint is deliberate: optimization must reuse the existing compiler and verification pipeline. It must not invent a second legality model, a second timing model, or a second emitter semantics layer.

---

## Scope

Current optimization is a library-level pipeline exposed through `rust_plc::optimization`.

Primary entrypoint:

```rust
use rust_plc::optimization::optimize_plc_source;
```

Input:
- original `.plc` source text

Output:
- ranked optimization candidates
- timing summary per candidate
- legality verdict per candidate
- stable emitted optimized PLC source

Current implementation is intentionally conservative:
- it optimizes on the preprocessed task AST
- it focuses on adjacent-step rewrites
- it reuses existing timing and verification logic
- it only rewrites `[tasks]` when emitting source

There is no CLI subcommand for optimization yet.

---

## Pipeline

The implemented flow is:

1. parse source and build `OptimizationContext`
2. preprocess program before searching for opportunities
3. `analyze_optimization_opportunities()`
4. `generate_candidate_rewrites()`
5. `evaluate_candidate_timing()`
6. `recheck_candidate_legality()`
7. `rank_candidates()`
8. `emit_optimized_plc()`

This keeps optimization aligned with the project-wide semantic rule:

`Parser -> AST -> Semantic -> IR -> Verification / Runtime Bridge / Codegen`

Optimization is not a side channel that bypasses that chain. It sits on top of the existing semantic closure and feeds candidates back through the same gates.

---

## Reused Infrastructure

### Timing

Candidate timing reuses the existing timing engine in `src/verification/timing.rs`.

Shared timing API:
- `estimate_program_timing(...)`
- `ProgramTimingEstimate`
- `ConcurrentTimingSummary`
- `StepTimingEstimate`

This means optimization timing is derived from the same task/step semantics already used elsewhere, including:
- sequential timing
- concurrent timing
- `wait`
- `delay`
- `timeout`
- `repeat` expansion

### Legality

Candidate legality reuses the existing semantic and verification pipeline instead of defining custom optimization-only checks.

For each candidate:
- `preprocess_program(...)`
- `build_topology_graph(...)`
- `build_constraint_set(...)`
- `build_state_machine(...)`
- `verify_all(...)`

That reuse is the critical design point. If a candidate fails, it fails under the same rules as ordinary PLC compilation.

### Emission

Candidate source emission preserves the original non-task prefix and only re-renders the `[tasks]` section.

That keeps these sections stable:
- `[topology]`
- `[constraints]`

And it avoids pretending optimization owns unrelated source formatting.

---

## Current Opportunity Classes

The phase-1 analyzer currently detects these conservative opportunities:

- reorder adjacent independent steps
- parallelize adjacent independent steps
- merge redundant waits
- merge adjacent delays
- replace simple timeout recovery route

The rewrite stage currently generates candidates for those same classes.

The ranking stage sorts candidates by:

1. legal before illegal
2. lower global nominal time first
3. fewer wait points
4. smaller change cost
5. stable id tie-break

---

## Library Usage

Minimal usage example:

```rust
use rust_plc::optimization::optimize_plc_source;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string("examples/project_scaffold_demo/plc/main.plc")?;
    let candidates = optimize_plc_source(&source)?;

    for candidate in candidates.iter().take(3) {
        println!(
            "{} legal={} nominal_ms={} waits={} rewrite={}",
            candidate.id,
            candidate.legality.is_legal,
            candidate.timing.global_nominal_ms,
            candidate.wait_points_after,
            candidate.rewrite.summary
        );
    }

    Ok(())
}
```

Useful candidate fields:
- `rewrite.summary`
- `timing.global_nominal_ms`
- `timing.global_worst_case_ms`
- `legality.is_legal`
- `legality.diagnostics`
- `source`

---

## Current Boundaries

These are intentional boundaries of the first version:

- no CLI command yet
- no optimization-specific verification rules
- no raw-source text surgery across topology or constraints
- no broad global scheduling search
- no speculative semantic changes in runtime or codegen

If future optimization needs richer transformations, the semantic shape must move up into IR or verified models first. It should not be guessed inside the optimization layer.

---

## Related Files

- `src/optimization/mod.rs`
- `src/optimization/analyzer.rs`
- `src/optimization/rewrite.rs`
- `src/optimization/timing.rs`
- `src/optimization/ranker.rs`
- `src/optimization/emitter.rs`
- `src/verification/timing.rs`
- `docs/已实现/plc_optimization_architecture_spec.md`

---

## Regression Coverage

Optimization-specific tests:

```bash
cargo test optimization::
```

Timing reuse regression:

```bash
cargo test verification::timing::tests::concurrent_worst_case_analysis_distinguishes_task_local_and_global_completion
```
