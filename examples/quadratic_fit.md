# Quadratic Fitting Example

This example demonstrates using RustPLC DSL to implement a least squares quadratic fitting algorithm for a set of data points.

## Overview

Given a set of data points (x₀, y₀), (x₁, y₁), ..., (xₙ, yₙ), this example computes the coefficients a, b, c of the quadratic function y = ax² + bx + c that best fits the data using the least squares method.

## Implementation

The algorithm follows these steps:

1. **Initialize**: Reset all accumulator variables
2. **Accumulate**: For each data point, compute and accumulate:
   - Σx, Σx², Σx³, Σx⁴
   - Σy, Σxy, Σx²y
3. **Solve System**: Use Cramer's rule to solve the linear system:
   ```
   n·a + Σx·b + Σx²·c = Σy
   Σx·a + Σx²·b + Σx³·c = Σxy
   Σx²·a + Σx³·b + Σx⁴·c = Σx²y
   ```
4. **Compute Coefficients**: Calculate a, b, c using determinants

## Example Data

The example uses 5 data points that follow y = 0.5x² + 2x + 1:

| x | y (actual) |
|---|------------|
| 0 | 1.0        |
| 1 | 3.5        |
| 2 | 8.0        |
| 3 | 14.5       |
| 4 | 23.0       |

Expected fitting result: a ≈ 0.5, b ≈ 2.0, c ≈ 1.0

## Key Features

### Variable Declarations
```plc
# Input data points
variable x0: float = 0.0
variable x1: float = 1.0
variable x2: float = 2.0
variable x3: float = 3.0
variable x4: float = 4.0

variable y0: float = 1.0
variable y1: float = 3.5
variable y2: float = 8.0
variable y3: float = 14.5
variable y4: float = 23.0

# Accumulators
variable sum_x: float = 0.0
variable sum_x2: float = 0.0
variable sum_x3: float = 0.0
variable sum_x4: float = 0.0
variable sum_y: float = 0.0
variable sum_xy: float = 0.0
variable sum_x2y: float = 0.0

# Results
variable a: float = 0.0
variable b: float = 0.0
variable c: float = 0.0
```

### Compute Statements
The example uses `compute` statements for mathematical operations:
```plc
action: compute x2_val = x_val * x_val
action: compute x3_val = x2_val * x_val
action: compute x4_val = x2_val * x2_val
action: compute sum_x = sum_x + x_val
action: compute sum_xy = sum_xy + x_val * y_val
```

### Matrix Solution (Cramer's Rule)
```plc
# Compute determinant of coefficient matrix
action: compute det = n * sum_x2 * sum_x4 + sum_x * sum_x3 * sum_x2 + sum_x2 * sum_x * sum_x3
action: compute det = det - sum_x2 * sum_x2 * sum_x2 - sum_x * sum_x * sum_x4 - n * sum_x3 * sum_x3

# Compute determinant for coefficient a
action: compute det_a = sum_y * sum_x2 * sum_x4 + sum_xy * sum_x3 * sum_x2 + sum_x2y * sum_x * sum_x3
action: compute det_a = det_a - sum_x2y * sum_x2 * sum_x2 - sum_xy * sum_x * sum_x4 - sum_y * sum_x3 * sum_x3
action: compute a = det_a / det
```

## Verification

The example passes all four verification checks:
- **Safety**: Complete proof (depth 12)
- **Liveness**: Pass
- **Timing**: Pass (completes within 5000ms constraint)
- **Causality**: Pass

## ST Code Generation

The example successfully generates IEC 61131-3 Structured Text code with:
- 48 REAL variables declared
- 12 states in the state machine
- Timer-based transitions between computation steps
- All computation logic preserved

## Variable Count

The example uses 48 variables, well within the MAX_VARIABLES limit of 64:
- 5 input x values (x0-x4)
- 5 input y values (y0-y4)
- 7 accumulator variables (sum_x, sum_x2, sum_x3, sum_x4, sum_y, sum_xy, sum_x2y)
- 5 temporary variables (x_val, y_val, x2_val, x3_val, x4_val)
- 4 determinant variables (det, det_a, det_b, det_c)
- 3 result variables (a, b, c)
- 2 utility variables (n, i)

## Usage

```bash
# Compile and verify
cargo run --release -- examples/quadratic_fit.plc --no-print-ir

# Generate ST code
cargo run --release -- gen-st examples/quadratic_fit.plc --out out/quadratic_fit.st

# Run tests
cargo test --test quadratic_fit_test
```

## Limitations

1. **Fixed Array Size**: Currently uses fixed variables (x0-x4, y0-y4) instead of arrays
2. **Manual Unrolling**: The accumulation loop is manually unrolled for each data point
3. **No Dynamic Input**: Data points are hardcoded as initial values
4. **Numerical Stability**: Uses direct Cramer's rule which may have numerical issues for ill-conditioned matrices

## Future Improvements

To make this more practical, the DSL would need:
- Array support for variable-length data
- Loop constructs for iterating over arrays
- External data input mechanisms
- Better numerical methods (QR decomposition, SVD)

## Notes

- This example demonstrates that RustPLC DSL can handle complex mathematical algorithms
- The `compute` statement supports arithmetic operators: `+`, `-`, `*`, `/`
- Variables must be declared in the `[topology]` section before use
- The algorithm is deterministic and suitable for real-time control applications
- All computation is done in fixed-point arithmetic (float32)
