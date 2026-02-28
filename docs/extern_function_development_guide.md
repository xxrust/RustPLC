# Extern Function Development Guide (Phase 1)

Status: **Published for rollout (US-018)**

This guide explains how to design extern contracts, implement Rust handlers, and write DSL flows that keep deterministic control logic in RustPLC while offloading heavy math to Rust.

For the frozen grammar/contract surface, always treat `docs/extern_function_mvp_spec.md` as the source of truth.

## 1) Development workflow

1. Declare extern signatures in `[topology]` with explicit `pure` and `time_bound_us`.
2. Implement/register Rust handlers in `ExternFunctionRegistry` (prefer `register_extern_function!`).
3. Call externs from task `action: call ... -> ...` steps only.
4. Execute runtime with extern-aware APIs (`tick_with_extern*`), not plain `tick*`.
5. Add semantic + runtime tests for signature, contract, and error handling paths.

## 2) Contract design best practices

- Keep `pure: true` whenever possible so verification and causality propagation remain deterministic.
- Keep signatures scalar (`bool`/`int`/`float`) and avoid hidden unit ambiguity; document units in parameter names (`temp_c`, `dt_s`).
- Set `time_bound_us` to a measured worst-case plus margin, then validate against scheduler `tick_ms` budget.
- Add `input_range`/`output_range` where safety-critical values can drift (sensor scaling, actuator commands).
- Prefer explicit tuple returns for multi-output computations (`-> (a, b, c)`) instead of encoding outputs into global state.
- For non-pure externs, isolate usage to controlled branches and avoid cross-branch concurrent invocation.

## 3) Rust implementation checklist

- Register extern metadata and function together so DSL stubs/signatures do not drift.
- Return structured runtime errors instead of panicking; let runtime map them to deterministic tick errors.
- Use `ExternFunctionRegistry::with_time_source` in tests to validate timeout behavior deterministically.
- For built-in/stateful functions (for example PID), expose a reset helper for deterministic multi-test runs.

## 4) Practical DSL examples (10)

### Example 1: Basic float addition (pure)

```dsl
[topology]
variable x: float = 1.5
variable y: float = 2.0
variable sum: float = 0.0

extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
    pure: true
    time_bound_us: 10
}

[task.main]
steps:
    - action: call add(x, y) -> sum
```

### Example 2: Integer scaling (pure)

```dsl
[topology]
variable pulses: int = 320
variable gear_ratio: int = 4
variable output_pulses: int = 0

extern function multiply(a: int, b: int) -> int {
    rust_module: "math::basic"
    pure: true
    time_bound_us: 10
}

[task.main]
steps:
    - action: call multiply(pulses, gear_ratio) -> output_pulses
```

### Example 3: Quadratic fit coefficients (pure tuple return)

```dsl
[topology]
variable x1: float = -2.0
variable x2: float = -1.0
variable x3: float = 0.0
variable x4: float = 1.0
variable x5: float = 2.0
variable y1: float = 3.0
variable y2: float = 1.0
variable y3: float = 1.0
variable y4: float = 3.0
variable y5: float = 7.0
variable coef_a: float = 0.0
variable coef_b: float = 0.0
variable coef_c: float = 0.0

extern function quadratic_fit(
    x1: float, x2: float, x3: float, x4: float, x5: float,
    y1: float, y2: float, y3: float, y4: float, y5: float
) -> (float, float, float) {
    rust_module: "math::fit"
    pure: true
    time_bound_us: 80
}

[task.fit]
steps:
    - action: call quadratic_fit(x1, x2, x3, x4, x5, y1, y2, y3, y4, y5) -> (coef_a, coef_b, coef_c)
```

### Example 4: PID loop output (non-pure, stateful)

```dsl
[topology]
variable err: float = 0.12
variable kp: float = 2.0
variable ki: float = 0.6
variable kd: float = 0.1
variable dt: float = 0.02
variable control_out: float = 0.0

extern function pid_update(error: float, kp: float, ki: float, kd: float, dt: float) -> float {
    rust_module: "control::pid"
    pure: false
    time_bound_us: 40
}

[task.loop]
steps:
    - action: call pid_update(err, kp, ki, kd, dt) -> control_out
```

### Example 5: PID output + feedforward bias chain

```dsl
[topology]
variable err: float = 0.12
variable kp: float = 2.0
variable ki: float = 0.6
variable kd: float = 0.1
variable dt: float = 0.02
variable pid_out: float = 0.0
variable ff_bias: float = 0.05
variable command_out: float = 0.0

extern function pid_update(error: float, kp: float, ki: float, kd: float, dt: float) -> float {
    rust_module: "control::pid"
    pure: false
    time_bound_us: 40
}

extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
    pure: true
    time_bound_us: 10
}

[task.loop]
steps:
    - action: call pid_update(err, kp, ki, kd, dt) -> pid_out
    - action: call add(pid_out, ff_bias) -> command_out
```

### Example 6: Extern error fallback with `last_error`

```dsl
[topology]
variable measurement: float = 0.0
variable filtered: float = 0.0
variable fallback: float = 0.0
variable last_error: int = 0

extern function filter(meas: float) -> float {
    rust_module: "control::filter"
    pure: false
    time_bound_us: 30
}

[tasks]
task main:
    step invoke:
        action: call filter(measurement) -> filtered
    on_complete: goto check

task check:
    step branch:
        wait: last_error == 0
        timeout: 1ms -> goto use_fallback
    on_complete: goto done

task use_fallback:
    step set_value:
        action: set filtered = fallback

task done:
    step hold:
        wait: last_error == 0
        timeout: 10ms -> goto done
```

### Example 7: Sensor normalization with range-protected output

```dsl
[topology]
variable adc_raw: float = 2450.0
variable pressure_bar: float = 0.0

extern function normalize_pressure(raw: float) -> float {
    rust_module: "io::normalize"
    pure: true
    time_bound_us: 12
    input_range: [0.0, 4095.0]
    output_range: [0.0, 16.0]
}

[task.main]
steps:
    - action: call normalize_pressure(adc_raw) -> pressure_bar
```

### Example 8: Interlock verdict function (bool return)

```dsl
[topology]
variable guard_closed: bool = true
variable e_stop_ok: bool = true
variable can_run: bool = false

extern function interlock_ok(guard_closed: bool, e_stop_ok: bool) -> bool {
    rust_module: "safety::interlock"
    pure: true
    time_bound_us: 6
}

[task.main]
steps:
    - action: call interlock_ok(guard_closed, e_stop_ok) -> can_run
```

### Example 9: No-argument extern for seeded startup mode

```dsl
[topology]
variable boot_mode: int = 0

extern function read_boot_mode() -> int {
    rust_module: "platform::boot"
    pure: false
    time_bound_us: 8
}

[task.startup]
steps:
    - action: call read_boot_mode() -> boot_mode
```

### Example 10: Retry once then escalate

```dsl
[topology]
variable command: float = 1.0
variable result: float = 0.0
variable last_error: int = 0

extern function flaky_apply(x: float) -> float {
    rust_module: "actuator::driver"
    pure: false
    time_bound_us: 30
}

[tasks]
task main:
    step attempt_1:
        action: call flaky_apply(command) -> result
    on_complete: goto check_1

task check_1:
    step branch:
        wait: last_error == 0
        timeout: 1ms -> goto retry
    on_complete: goto success

task retry:
    step attempt_2:
        action: call flaky_apply(command) -> result
    on_complete: goto check_2

task check_2:
    step branch:
        wait: last_error == 0
        timeout: 1ms -> goto fault

task success:
    step hold:
        wait: last_error == 0
        timeout: 10ms -> goto success

task fault:
    step hold:
        wait: last_error != 0
        timeout: 10ms -> goto fault
```

## 5) Migration guidance

Use this sequence when migrating expression-heavy DSL logic to extern calls:

1. Identify compute hotspots (matrix fitting, nonlinear control, expensive filtering).
2. Keep control-plane decisions (`wait`, `timeout`, `goto`, safety constraints) in DSL; move only numeric kernels to externs.
3. Introduce extern declarations in `[topology]` with conservative `time_bound_us` and scalar signatures.
4. Replace expression-context pseudo-calls with `action: call ... -> ...` and declare return variables explicitly.
5. If failures must be recoverable in control flow, add `[topology] variable last_error: int` and run with `tick_with_extern_error_code`.
6. Add integration tests that compile DSL to runtime and assert both success and contract-violation paths.

## 6) Known limitations (Phase 1)

- Only scalar argument/return types are supported (`bool`, `int`, `float`) plus scalar tuples for returns.
- No overloads, generics, variadics, callbacks, or named/default arguments.
- Extern calls are action-only (not allowed in expression context).
- Non-pure extern concurrency checks are conservative and may reject ambiguous cross-branch usage.
- Tick budget checks are static worst-case sums of `time_bound_us`; tune bounds carefully to avoid false budget failures.
- Runtime without extern handler (`tick*`) fails intentionally for programs containing `CallExtern` actions.

## 7) Reference links

- Frozen grammar and syntax contract: `docs/extern_function_mvp_spec.md`
- Runtime contract enforcement and registry APIs: `src/extern_functions.rs`
- Runtime extern execution APIs: `crates/runtime-core/src/lib.rs`
- End-to-end extern integration tests: `tests/runtime_bridge_us006.rs`
