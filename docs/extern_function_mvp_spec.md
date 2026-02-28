# Extern Function MVP Syntax (Frozen)

Status: **MVP frozen for implementation (US-001)**

This document defines the only supported DSL surface for Phase 1 extern integration.
Parser/runtime work must follow this contract exactly until a later RFC updates it.

For rollout-oriented implementation guidance, practical usage patterns, migration notes, and
known limitations, see `docs/extern_function_development_guide.md`.

## 1) Supported Syntax

### 1.1 Extern function declaration

Extern functions are declared in `[topology]`.

```dsl
extern function <name>(<param_list>) -> <return_spec> {
    rust_module: "<module_path>"
    pure: <true|false>
    time_bound_us: <positive_integer>
}
```

Rules:
- `<name>`: identifier, unique in topology scope.
- `<param_list>`: zero or more typed parameters.
- `<return_spec>`: either a single scalar type or tuple of scalar types.
- Required contract fields (all mandatory):
  - `rust_module`
  - `pure`
  - `time_bound_us`

Scalar types in MVP:
- `bool`
- `int`
- `float`

### 1.2 Extern call action

Extern calls are only legal in task steps as an `action`.

```dsl
action: call <name>(<arg_list>) -> <binding>
```

Binding forms:
- Single return binding: `-> out_var`
- Tuple return binding: `-> (out_a, out_b, out_c)`

## 2) Explicitly Out of Scope (MVP)

The following are **not supported** in MVP:
- Function overloads (same name, different signatures)
- Variadic arguments
- Generic type parameters
- Callback/function-pointer parameters
- Array/list/map/struct return or argument types
- Named arguments at call site
- Default parameter values
- Expression-context call syntax (e.g. `x = add(a, b)`)

## 3) Examples (Valid + Invalid)

### Valid Examples

1) Valid single-return declaration and call

```dsl
[topology]
extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
    pure: true
    time_bound_us: 10
}

[task.main]
steps:
    - action: call add(x, y) -> sum
```

2) Valid tuple return declaration and call

```dsl
[topology]
extern function split(v: float) -> (float, float) {
    rust_module: "math::split"
    pure: true
    time_bound_us: 15
}

[task.main]
steps:
    - action: call split(input) -> (lo, hi)
```

3) Valid non-pure declaration

```dsl
[topology]
extern function pid_update(err: float, kp: float, ki: float, kd: float, dt: float) -> float {
    rust_module: "control::pid"
    pure: false
    time_bound_us: 20
}
```

4) Valid bool/int signature

```dsl
[topology]
extern function saturate(v: int, enabled: bool) -> int {
    rust_module: "logic::limit"
    pure: true
    time_bound_us: 8
}
```

5) Valid no-arg extern function

```dsl
[topology]
extern function read_seed() -> int {
    rust_module: "platform::seed"
    pure: false
    time_bound_us: 5
}
```

6) Valid 3-value tuple binding

```dsl
[task.fit]
steps:
    - action: call quadratic_fit(x0, x1, x2, y0, y1, y2) -> (a, b, c)
```

### Invalid Examples

7) Invalid: missing required contract field `rust_module`

```dsl
[topology]
extern function add(a: float, b: float) -> float {
    pure: true
    time_bound_us: 10
}
```

8) Invalid: unsupported overload (duplicate name)

```dsl
[topology]
extern function add(a: float, b: float) -> float { rust_module: "m::a" pure: true time_bound_us: 10 }
extern function add(a: int, b: int) -> int { rust_module: "m::a" pure: true time_bound_us: 10 }
```

9) Invalid: unsupported variadic parameter

```dsl
[topology]
extern function mean(values: float...) -> float {
    rust_module: "math::stats"
    pure: true
    time_bound_us: 30
}
```

10) Invalid: unsupported generics

```dsl
[topology]
extern function identity<T>(x: T) -> T {
    rust_module: "util::id"
    pure: true
    time_bound_us: 3
}
```

11) Invalid: call used in expression context

```dsl
[task.main]
steps:
    - action: set result = add(x, y)
```

12) Invalid: tuple-return function bound to single variable

```dsl
[task.main]
steps:
    - action: call split(value) -> only_one
```

## 4) Compatibility Notes

- This spec is intentionally narrow to unblock parser/semantic/runtime increments.
- Any syntax beyond this file must be treated as future extension work and requires a new RFC/PRD update.

## 5) Runtime Extern Error Handling Pattern (US-014)

Extern runtime failures can be surfaced into a normal DSL variable flow by declaring a topology variable
named `last_error` (type `int`) and running the tick with runtime-core's error-code capture API:

- Host runtime API: `Runtime::tick_with_extern_error_code(...)`
- Recommended mapping helper: `rust_plc::extern_functions::extern_runtime_error_code(...)`
- `last_error == 0` means success; non-zero means extern failure code.

Stable code mapping (Phase 1):
- `0`: success (`EXTERN_ERROR_CODE_OK`)
- `1`: function not found
- `2`: invalid arg count
- `3`: input out of range
- `4`: output out of range
- `5`: timeout
- `6`: runtime error

Example branching pattern (retry/fallback/error tasks) without new DSL keywords:

```dsl
[topology]
variable last_error: int = 0

[tasks]
task main:
    step invoke:
        action: call flaky_fn(x) -> y
    on_complete: goto check_first

task check_first:
    step branch:
        wait: last_error == 0
        timeout: 1ms -> goto retry
    on_complete: goto success

task retry:
    step invoke_retry:
        action: call flaky_fn(x) -> y
    on_complete: goto check_second

task check_second:
    step branch:
        wait: last_error == 0
        timeout: 1ms -> goto error
    on_complete: goto success
```
