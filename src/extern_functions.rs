use crate::ir::{ExternFunctionContract, VariableType};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;
use thiserror::Error;

pub type ExternFunctionImpl = Arc<dyn Fn(&[f32]) -> Result<Vec<f32>, String> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueRange {
    pub min: f32,
    pub max: f32,
}

impl ValueRange {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    fn contains(&self, value: f32) -> bool {
        value >= self.min && value <= self.max
    }
}

#[derive(Clone)]
pub struct ExternFunctionInfo {
    pub name: String,
    pub param_types: Vec<VariableType>,
    pub return_types: Vec<VariableType>,
    pub contract: ExternFunctionContract,
    pub input_ranges: Option<Vec<ValueRange>>,
    pub output_ranges: Option<Vec<ValueRange>>,
    function: ExternFunctionImpl,
}

impl ExternFunctionInfo {
    pub fn new(
        name: impl Into<String>,
        param_types: Vec<VariableType>,
        return_types: Vec<VariableType>,
        contract: ExternFunctionContract,
        function: impl Fn(&[f32]) -> Result<Vec<f32>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            param_types,
            return_types,
            contract,
            input_ranges: None,
            output_ranges: None,
            function: Arc::new(function),
        }
    }

    pub fn with_input_ranges(mut self, ranges: Vec<ValueRange>) -> Self {
        self.input_ranges = Some(ranges);
        self
    }

    pub fn with_output_ranges(mut self, ranges: Vec<ValueRange>) -> Self {
        self.output_ranges = Some(ranges);
        self
    }

    fn invoke(&self, args: &[f32]) -> Result<Vec<f32>, String> {
        (self.function)(args)
    }
}

impl fmt::Debug for ExternFunctionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternFunctionInfo")
            .field("name", &self.name)
            .field("param_types", &self.param_types)
            .field("return_types", &self.return_types)
            .field("contract", &self.contract)
            .field("input_ranges", &self.input_ranges)
            .field("output_ranges", &self.output_ranges)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Error, PartialEq)]
pub enum ExternRuntimeError {
    #[error("extern function {name} is not registered")]
    FunctionNotFound { name: String },
    #[error("extern function {name} is already registered")]
    DuplicateFunction { name: String },
    #[error("extern function {function} expected {expected} args, got {got}")]
    InvalidArgCount {
        function: String,
        expected: usize,
        got: usize,
    },
    #[error(
        "extern function {function} input arg #{arg_index}={value} out of range [{min}, {max}]"
    )]
    InputOutOfRange {
        function: String,
        arg_index: usize,
        value: f32,
        min: f32,
        max: f32,
    },
    #[error(
        "extern function {function} output #{result_index}={value} out of range [{min}, {max}]"
    )]
    OutputOutOfRange {
        function: String,
        result_index: usize,
        value: f32,
        min: f32,
        max: f32,
    },
    #[error(
        "extern function {function} timed out: elapsed {elapsed_us}us exceeds limit {limit_us}us"
    )]
    TimeoutExceeded {
        function: String,
        elapsed_us: u64,
        limit_us: u64,
    },
    #[error("extern function {function} runtime error: {message}")]
    RuntimeError { function: String, message: String },
    #[error(
        "extern function {function} has invalid {kind} range count: expected {expected}, got {got}"
    )]
    InvalidRangeCount {
        function: String,
        kind: &'static str,
        expected: usize,
        got: usize,
    },
    #[error("extern function {function} has invalid {kind} range #{index}: min {min} > max {max}")]
    InvalidRangeBounds {
        function: String,
        kind: &'static str,
        index: usize,
        min: f32,
        max: f32,
    },
}

pub struct ExternFunctionRegistry {
    functions: HashMap<String, ExternFunctionInfo>,
    time_source: Arc<dyn Fn() -> u64 + Send + Sync>,
    builtin_pid_state: Arc<Mutex<PidRuntimeState>>,
}

impl Default for ExternFunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ExternFunctionRegistry {
    pub fn new() -> Self {
        let start = Instant::now();
        let pid_state = Arc::new(Mutex::new(PidRuntimeState::default()));
        let mut registry = Self {
            functions: HashMap::new(),
            time_source: Arc::new(move || {
                let elapsed = start.elapsed().as_micros();
                elapsed.min(u128::from(u64::MAX)) as u64
            }),
            builtin_pid_state: pid_state,
        };
        registry
            .register_builtin_math_functions()
            .expect("built-in extern functions should register");
        registry
            .register_builtin_control_functions()
            .expect("built-in control extern functions should register");
        registry
    }

    pub fn with_time_source(time_source: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self {
            functions: HashMap::new(),
            time_source: Arc::new(time_source),
            builtin_pid_state: Arc::new(Mutex::new(PidRuntimeState::default())),
        }
    }

    pub fn register_builtin_math_functions(&mut self) -> Result<(), ExternRuntimeError> {
        self.register(builtin_add_info())?;
        self.register(builtin_multiply_info())?;
        self.register(builtin_quadratic_fit_info())?;
        Ok(())
    }

    pub fn register_builtin_control_functions(&mut self) -> Result<(), ExternRuntimeError> {
        self.register(builtin_pid_update_info(Arc::clone(&self.builtin_pid_state)))?;
        Ok(())
    }

    pub fn reset_builtin_pid_state(&self) {
        let mut state = self
            .builtin_pid_state
            .lock()
            .expect("pid builtin state lock should not be poisoned");
        *state = PidRuntimeState::default();
    }

    pub fn register(&mut self, info: ExternFunctionInfo) -> Result<(), ExternRuntimeError> {
        self.validate_ranges(&info)?;
        if self.functions.contains_key(&info.name) {
            return Err(ExternRuntimeError::DuplicateFunction {
                name: info.name.clone(),
            });
        }
        self.functions.insert(info.name.clone(), info);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ExternFunctionInfo> {
        self.functions.get(name)
    }

    pub fn call(&self, name: &str, args: &[f32]) -> Result<Vec<f32>, ExternRuntimeError> {
        let info =
            self.functions
                .get(name)
                .ok_or_else(|| ExternRuntimeError::FunctionNotFound {
                    name: name.to_string(),
                })?;
        let function = info.name.clone();

        if args.len() != info.param_types.len() {
            return Err(ExternRuntimeError::InvalidArgCount {
                function,
                expected: info.param_types.len(),
                got: args.len(),
            });
        }

        if let Some(ranges) = &info.input_ranges {
            for (arg_index, (arg, range)) in args.iter().zip(ranges.iter()).enumerate() {
                if !range.contains(*arg) {
                    return Err(ExternRuntimeError::InputOutOfRange {
                        function: info.name.clone(),
                        arg_index,
                        value: *arg,
                        min: range.min,
                        max: range.max,
                    });
                }
            }
        }

        let started_us = (self.time_source)();
        let result = info
            .invoke(args)
            .map_err(|message| ExternRuntimeError::RuntimeError {
                function: info.name.clone(),
                message,
            })?;
        let elapsed_us = (self.time_source)().saturating_sub(started_us);

        if elapsed_us > info.contract.time_bound_us {
            return Err(ExternRuntimeError::TimeoutExceeded {
                function: info.name.clone(),
                elapsed_us,
                limit_us: info.contract.time_bound_us,
            });
        }

        if result.len() != info.return_types.len() {
            return Err(ExternRuntimeError::RuntimeError {
                function: info.name.clone(),
                message: format!(
                    "invalid return count: expected {}, got {}",
                    info.return_types.len(),
                    result.len()
                ),
            });
        }

        if let Some(ranges) = &info.output_ranges {
            for (result_index, (value, range)) in result.iter().zip(ranges.iter()).enumerate() {
                if !range.contains(*value) {
                    return Err(ExternRuntimeError::OutputOutOfRange {
                        function: info.name.clone(),
                        result_index,
                        value: *value,
                        min: range.min,
                        max: range.max,
                    });
                }
            }
        }

        Ok(result)
    }

    fn validate_ranges(&self, info: &ExternFunctionInfo) -> Result<(), ExternRuntimeError> {
        Self::validate_range_set(
            info,
            info.input_ranges.as_ref(),
            info.param_types.len(),
            "input",
        )?;
        Self::validate_range_set(
            info,
            info.output_ranges.as_ref(),
            info.return_types.len(),
            "output",
        )?;
        Ok(())
    }

    fn validate_range_set(
        info: &ExternFunctionInfo,
        ranges: Option<&Vec<ValueRange>>,
        expected_len: usize,
        kind: &'static str,
    ) -> Result<(), ExternRuntimeError> {
        let Some(ranges) = ranges else {
            return Ok(());
        };

        if ranges.len() != expected_len {
            return Err(ExternRuntimeError::InvalidRangeCount {
                function: info.name.clone(),
                kind,
                expected: expected_len,
                got: ranges.len(),
            });
        }

        for (index, range) in ranges.iter().enumerate() {
            if range.min > range.max {
                return Err(ExternRuntimeError::InvalidRangeBounds {
                    function: info.name.clone(),
                    kind,
                    index,
                    min: range.min,
                    max: range.max,
                });
            }
        }

        Ok(())
    }
}

const BUILTIN_ADD: &str = "add";
const BUILTIN_MULTIPLY: &str = "multiply";
const BUILTIN_QUADRATIC_FIT: &str = "quadratic_fit";
const BUILTIN_PID_UPDATE: &str = "pid_update";
const QUADRATIC_FIT_POINT_COUNT: usize = 5;
const QUADRATIC_FIT_ARG_COUNT: usize = QUADRATIC_FIT_POINT_COUNT * 2;
const PID_UPDATE_ARG_COUNT: usize = 5;
const BUILTIN_TIME_BOUND_US: u64 = 1_000_000;

#[derive(Debug, Default)]
struct PidRuntimeState {
    integral_error: f32,
    previous_error: f32,
    has_previous_error: bool,
}

fn builtin_add_info() -> ExternFunctionInfo {
    ExternFunctionInfo::new(
        BUILTIN_ADD,
        vec![VariableType::Float, VariableType::Float],
        vec![VariableType::Float],
        ExternFunctionContract {
            rust_module: "math::basic".to_string(),
            pure: true,
            time_bound_us: BUILTIN_TIME_BOUND_US,
        },
        builtin_add,
    )
}

fn builtin_multiply_info() -> ExternFunctionInfo {
    ExternFunctionInfo::new(
        BUILTIN_MULTIPLY,
        vec![VariableType::Float, VariableType::Float],
        vec![VariableType::Float],
        ExternFunctionContract {
            rust_module: "math::basic".to_string(),
            pure: true,
            time_bound_us: BUILTIN_TIME_BOUND_US,
        },
        builtin_multiply,
    )
}

fn builtin_quadratic_fit_info() -> ExternFunctionInfo {
    ExternFunctionInfo::new(
        BUILTIN_QUADRATIC_FIT,
        vec![VariableType::Float; QUADRATIC_FIT_ARG_COUNT],
        vec![
            VariableType::Float,
            VariableType::Float,
            VariableType::Float,
        ],
        ExternFunctionContract {
            rust_module: "math::fitting".to_string(),
            pure: true,
            time_bound_us: BUILTIN_TIME_BOUND_US,
        },
        builtin_quadratic_fit,
    )
}

fn builtin_pid_update_info(pid_state: Arc<Mutex<PidRuntimeState>>) -> ExternFunctionInfo {
    ExternFunctionInfo::new(
        BUILTIN_PID_UPDATE,
        vec![VariableType::Float; PID_UPDATE_ARG_COUNT],
        vec![VariableType::Float],
        ExternFunctionContract {
            rust_module: "control::pid".to_string(),
            pure: false,
            time_bound_us: BUILTIN_TIME_BOUND_US,
        },
        move |args| builtin_pid_update(args, &pid_state),
    )
}

fn builtin_add(args: &[f32]) -> Result<Vec<f32>, String> {
    Ok(vec![args[0] + args[1]])
}

fn builtin_multiply(args: &[f32]) -> Result<Vec<f32>, String> {
    Ok(vec![args[0] * args[1]])
}

fn builtin_quadratic_fit(args: &[f32]) -> Result<Vec<f32>, String> {
    let x_values: Vec<f64> = args
        .iter()
        .take(QUADRATIC_FIT_POINT_COUNT)
        .map(|v| f64::from(*v))
        .collect();
    let y_values: Vec<f64> = args
        .iter()
        .skip(QUADRATIC_FIT_POINT_COUNT)
        .take(QUADRATIC_FIT_POINT_COUNT)
        .map(|v| f64::from(*v))
        .collect();
    let [a, b, c] = solve_quadratic_fit_coefficients(&x_values, &y_values)?;
    Ok(vec![a as f32, b as f32, c as f32])
}

fn builtin_pid_update(
    args: &[f32],
    state: &Arc<Mutex<PidRuntimeState>>,
) -> Result<Vec<f32>, String> {
    let error = args[0];
    let kp = args[1];
    let ki = args[2];
    let kd = args[3];
    let dt = args[4];

    if !dt.is_finite() || dt <= 0.0 {
        return Err("pid_update requires dt > 0".to_string());
    }

    let mut state = state
        .lock()
        .expect("pid builtin state lock should not be poisoned");
    state.integral_error += error * dt;
    let derivative_error = if state.has_previous_error {
        (error - state.previous_error) / dt
    } else {
        error / dt
    };
    let output = kp * error + ki * state.integral_error + kd * derivative_error;
    state.previous_error = error;
    state.has_previous_error = true;

    Ok(vec![output])
}

fn solve_quadratic_fit_coefficients(
    x_values: &[f64],
    y_values: &[f64],
) -> Result<[f64; 3], String> {
    let n = x_values.len() as f64;
    let (sum_x, sum_x2, sum_x3, sum_x4) =
        x_values
            .iter()
            .fold((0.0, 0.0, 0.0, 0.0), |(sx, sx2, sx3, sx4), &x| {
                let x2 = x * x;
                (sx + x, sx2 + x2, sx3 + x2 * x, sx4 + x2 * x2)
            });

    let sum_y = y_values.iter().sum::<f64>();
    let sum_xy = x_values
        .iter()
        .zip(y_values.iter())
        .map(|(x, y)| x * y)
        .sum::<f64>();
    let sum_x2y = x_values
        .iter()
        .zip(y_values.iter())
        .map(|(x, y)| x * x * y)
        .sum::<f64>();

    let det = n * sum_x2 * sum_x4 + sum_x * sum_x3 * sum_x2 + sum_x2 * sum_x * sum_x3
        - sum_x2 * sum_x2 * sum_x2
        - sum_x * sum_x * sum_x4
        - n * sum_x3 * sum_x3;
    if det.abs() < 1e-10 {
        return Err("singular matrix: determinant too small".to_string());
    }

    let det_a = sum_y * sum_x2 * sum_x4 + sum_xy * sum_x3 * sum_x2 + sum_x2y * sum_x * sum_x3
        - sum_x2y * sum_x2 * sum_x2
        - sum_xy * sum_x * sum_x4
        - sum_y * sum_x3 * sum_x3;
    let det_b = n * sum_xy * sum_x4 + sum_x * sum_x2y * sum_x2 + sum_x2 * sum_y * sum_x3
        - sum_x2 * sum_xy * sum_x2
        - sum_x * sum_y * sum_x4
        - n * sum_x2y * sum_x3;
    let det_c = n * sum_x2 * sum_x2y + sum_x * sum_x3 * sum_y + sum_x2 * sum_x * sum_xy
        - sum_x2 * sum_x2 * sum_y
        - sum_x * sum_x * sum_x2y
        - n * sum_x3 * sum_xy;

    let a = det_a / det;
    let b = det_b / det;
    let c = det_c / det;

    if !a.is_finite() || !b.is_finite() || !c.is_finite() {
        return Err("quadratic fit produced non-finite coefficients".to_string());
    }

    Ok([a, b, c])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    fn contract(time_bound_us: u64) -> ExternFunctionContract {
        ExternFunctionContract {
            rust_module: "math::test".to_string(),
            pure: true,
            time_bound_us,
        }
    }

    fn add_info() -> ExternFunctionInfo {
        ExternFunctionInfo::new(
            "add",
            vec![VariableType::Float, VariableType::Float],
            vec![VariableType::Float],
            contract(100),
            |args| Ok(vec![args[0] + args[1]]),
        )
    }

    #[test]
    fn register_get_and_call_succeeds() {
        let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
        registry
            .register(add_info())
            .expect("register should succeed");

        let add = registry.get("add").expect("function should be registered");
        assert_eq!(add.contract.rust_module, "math::test");

        let result = registry.call("add", &[1.5, 2.0]).expect("call should pass");
        assert_eq!(result, vec![3.5]);
    }

    #[test]
    fn call_returns_deterministic_error_for_missing_function() {
        let registry = ExternFunctionRegistry::with_time_source(|| 0);
        let err = registry
            .call("missing", &[])
            .expect_err("missing function should fail");

        assert_eq!(
            err,
            ExternRuntimeError::FunctionNotFound {
                name: "missing".to_string()
            }
        );
    }

    #[test]
    fn call_reports_invalid_argument_count() {
        let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
        registry
            .register(add_info())
            .expect("register should succeed");

        let err = registry
            .call("add", &[1.0])
            .expect_err("wrong arg count should fail");
        assert_eq!(
            err,
            ExternRuntimeError::InvalidArgCount {
                function: "add".to_string(),
                expected: 2,
                got: 1,
            }
        );
    }

    #[test]
    fn call_reports_input_and_output_range_errors() {
        let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
        registry
            .register(
                ExternFunctionInfo::new(
                    "bounded",
                    vec![VariableType::Float],
                    vec![VariableType::Float],
                    contract(100),
                    |_| Ok(vec![3.0]),
                )
                .with_input_ranges(vec![ValueRange::new(-1.0, 1.0)])
                .with_output_ranges(vec![ValueRange::new(-2.0, 2.0)]),
            )
            .expect("register should succeed");

        let input_err = registry
            .call("bounded", &[5.0])
            .expect_err("input out of range should fail");
        assert_eq!(
            input_err,
            ExternRuntimeError::InputOutOfRange {
                function: "bounded".to_string(),
                arg_index: 0,
                value: 5.0,
                min: -1.0,
                max: 1.0,
            }
        );

        let output_err = registry
            .call("bounded", &[1.0])
            .expect_err("output out of range should fail");
        assert_eq!(
            output_err,
            ExternRuntimeError::OutputOutOfRange {
                function: "bounded".to_string(),
                result_index: 0,
                value: 3.0,
                min: -2.0,
                max: 2.0,
            }
        );
    }

    #[test]
    fn call_reports_timeout_and_runtime_errors() {
        let ticks = Arc::new(AtomicU64::new(0));
        let clock = Arc::clone(&ticks);
        let mut timeout_registry = ExternFunctionRegistry::with_time_source(move || {
            let now = clock.load(Ordering::Relaxed);
            clock.store(now + 50, Ordering::Relaxed);
            now
        });
        timeout_registry
            .register(ExternFunctionInfo::new(
                "slow",
                vec![VariableType::Float],
                vec![VariableType::Float],
                contract(10),
                |args| Ok(vec![args[0]]),
            ))
            .expect("register should succeed");

        let timeout_err = timeout_registry
            .call("slow", &[1.0])
            .expect_err("timeout should fail");
        assert_eq!(
            timeout_err,
            ExternRuntimeError::TimeoutExceeded {
                function: "slow".to_string(),
                elapsed_us: 50,
                limit_us: 10,
            }
        );

        let mut runtime_registry = ExternFunctionRegistry::with_time_source(|| 0);
        runtime_registry
            .register(ExternFunctionInfo::new(
                "boom",
                vec![VariableType::Float],
                vec![VariableType::Float],
                contract(10),
                |_| Err("panic from extern".to_string()),
            ))
            .expect("register should succeed");

        let runtime_err = runtime_registry
            .call("boom", &[1.0])
            .expect_err("runtime error should fail");
        assert_eq!(
            runtime_err,
            ExternRuntimeError::RuntimeError {
                function: "boom".to_string(),
                message: "panic from extern".to_string(),
            }
        );
    }

    #[test]
    fn register_rejects_duplicate_names_and_invalid_range_config() {
        let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
        registry
            .register(add_info())
            .expect("first register should pass");

        let duplicate = registry
            .register(add_info())
            .expect_err("duplicate function should fail");
        assert_eq!(
            duplicate,
            ExternRuntimeError::DuplicateFunction {
                name: "add".to_string(),
            }
        );

        let invalid_count = registry
            .register(
                ExternFunctionInfo::new(
                    "bad_ranges",
                    vec![VariableType::Float],
                    vec![VariableType::Float],
                    contract(10),
                    |args| Ok(vec![args[0]]),
                )
                .with_input_ranges(vec![ValueRange::new(0.0, 1.0), ValueRange::new(0.0, 1.0)]),
            )
            .expect_err("range count mismatch should fail");
        assert_eq!(
            invalid_count,
            ExternRuntimeError::InvalidRangeCount {
                function: "bad_ranges".to_string(),
                kind: "input",
                expected: 1,
                got: 2,
            }
        );

        let invalid_bounds = registry
            .register(
                ExternFunctionInfo::new(
                    "bad_bounds",
                    vec![VariableType::Float],
                    vec![VariableType::Float],
                    contract(10),
                    |args| Ok(vec![args[0]]),
                )
                .with_output_ranges(vec![ValueRange::new(2.0, 1.0)]),
            )
            .expect_err("invalid range bounds should fail");
        assert_eq!(
            invalid_bounds,
            ExternRuntimeError::InvalidRangeBounds {
                function: "bad_bounds".to_string(),
                kind: "output",
                index: 0,
                min: 2.0,
                max: 1.0,
            }
        );
    }

    #[test]
    fn registry_bootstrap_registers_builtin_math_functions() {
        let registry = ExternFunctionRegistry::new();
        assert!(registry.get(BUILTIN_ADD).is_some());
        assert!(registry.get(BUILTIN_MULTIPLY).is_some());
        assert!(registry.get(BUILTIN_QUADRATIC_FIT).is_some());
        assert!(registry.get(BUILTIN_PID_UPDATE).is_some());
    }

    #[test]
    fn builtin_add_and_multiply_execute_successfully() {
        let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
        registry
            .register_builtin_math_functions()
            .expect("builtin registration should pass");

        let add = registry
            .call(BUILTIN_ADD, &[1.5, 2.0])
            .expect("add should pass");
        assert_eq!(add, vec![3.5]);

        let multiply = registry
            .call(BUILTIN_MULTIPLY, &[3.0, 4.0])
            .expect("multiply should pass");
        assert_eq!(multiply, vec![12.0]);
    }

    #[test]
    fn builtin_quadratic_fit_returns_coefficients_and_reports_singular_matrix() {
        let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
        registry
            .register_builtin_math_functions()
            .expect("builtin registration should pass");

        let fit = registry
            .call(
                BUILTIN_QUADRATIC_FIT,
                &[0.0, 1.0, 2.0, 3.0, 4.0, 1.0, 6.0, 15.0, 28.0, 45.0],
            )
            .expect("quadratic fit should pass");
        assert!(
            (fit[0] - 1.0).abs() < 1e-4
                && (fit[1] - 3.0).abs() < 1e-4
                && (fit[2] - 2.0).abs() < 1e-4,
            "unexpected coefficients: {fit:?}"
        );

        let singular = registry
            .call(
                BUILTIN_QUADRATIC_FIT,
                &[1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            )
            .expect_err("repeated x values should be singular");
        match singular {
            ExternRuntimeError::RuntimeError { function, message } => {
                assert_eq!(function, BUILTIN_QUADRATIC_FIT);
                assert!(
                    message.contains("singular matrix"),
                    "unexpected singular message: {message}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn builtin_math_functions_reject_invalid_argument_counts() {
        let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
        registry
            .register_builtin_math_functions()
            .expect("builtin registration should pass");

        let add_err = registry
            .call(BUILTIN_ADD, &[1.0])
            .expect_err("add arity mismatch should fail");
        assert_eq!(
            add_err,
            ExternRuntimeError::InvalidArgCount {
                function: BUILTIN_ADD.to_string(),
                expected: 2,
                got: 1,
            }
        );

        let multiply_err = registry
            .call(BUILTIN_MULTIPLY, &[1.0])
            .expect_err("multiply arity mismatch should fail");
        assert_eq!(
            multiply_err,
            ExternRuntimeError::InvalidArgCount {
                function: BUILTIN_MULTIPLY.to_string(),
                expected: 2,
                got: 1,
            }
        );

        let fit_err = registry
            .call(BUILTIN_QUADRATIC_FIT, &[0.0; 9])
            .expect_err("quadratic_fit arity mismatch should fail");
        assert_eq!(
            fit_err,
            ExternRuntimeError::InvalidArgCount {
                function: BUILTIN_QUADRATIC_FIT.to_string(),
                expected: QUADRATIC_FIT_ARG_COUNT,
                got: 9,
            }
        );
    }

    #[test]
    fn builtin_pid_update_supports_first_step_and_multistep_accumulation() {
        let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
        registry
            .register_builtin_control_functions()
            .expect("pid registration should pass");

        let first = registry
            .call(BUILTIN_PID_UPDATE, &[1.0, 2.0, 0.5, 0.25, 0.1])
            .expect("first pid update should pass");
        assert!(
            (first[0] - 4.55).abs() < 1e-5,
            "unexpected first step: {first:?}"
        );

        let second = registry
            .call(BUILTIN_PID_UPDATE, &[0.5, 2.0, 0.5, 0.25, 0.1])
            .expect("second pid update should pass");
        assert!(
            (second[0] - -0.175).abs() < 1e-5,
            "unexpected second step: {second:?}"
        );
    }

    #[test]
    fn builtin_pid_update_state_reset_is_deterministic_for_tests() {
        let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
        registry
            .register_builtin_control_functions()
            .expect("pid registration should pass");

        let _ = registry
            .call(BUILTIN_PID_UPDATE, &[1.0, 2.0, 0.5, 0.25, 0.1])
            .expect("first pid update should pass");
        let second_without_reset = registry
            .call(BUILTIN_PID_UPDATE, &[1.0, 2.0, 0.5, 0.25, 0.1])
            .expect("second pid update should pass");

        registry.reset_builtin_pid_state();
        let after_reset = registry
            .call(BUILTIN_PID_UPDATE, &[1.0, 2.0, 0.5, 0.25, 0.1])
            .expect("pid update after reset should pass");

        assert!((second_without_reset[0] - 2.1).abs() < 1e-5);
        assert!((after_reset[0] - 4.55).abs() < 1e-5);
    }
}
