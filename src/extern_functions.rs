use crate::ir::{ExternFunctionContract, VariableType};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
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
}

impl Default for ExternFunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ExternFunctionRegistry {
    pub fn new() -> Self {
        let start = Instant::now();
        Self {
            functions: HashMap::new(),
            time_source: Arc::new(move || {
                let elapsed = start.elapsed().as_micros();
                elapsed.min(u128::from(u64::MAX)) as u64
            }),
        }
    }

    pub fn with_time_source(time_source: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self {
            functions: HashMap::new(),
            time_source: Arc::new(time_source),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

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
}
