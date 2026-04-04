fn eval_expr(program: &ExprProgram, vars: &[f32; MAX_VARIABLES]) -> f32 {
    if program.len == 0 {
        return 0.0;
    }

    let mut stack = [0.0f32; MAX_EXPR_STACK];
    let mut sp = 0usize;
    for op in program.ops.iter().take(program.len as usize) {
        match *op {
            ExprOp::PushLiteral(v) => {
                if sp >= MAX_EXPR_STACK {
                    return 0.0;
                }
                stack[sp] = v;
                sp += 1;
            }
            ExprOp::PushVariable(idx) => {
                let idx = idx as usize;
                if idx >= MAX_VARIABLES || sp >= MAX_EXPR_STACK {
                    return 0.0;
                }
                stack[sp] = vars[idx];
                sp += 1;
            }
            ExprOp::Add => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] += stack[sp];
            }
            ExprOp::Sub => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] -= stack[sp];
            }
            ExprOp::Mul => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] *= stack[sp];
            }
            ExprOp::Div => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let rhs = stack[sp];
                if rhs == 0.0 {
                    return 0.0;
                }
                stack[sp - 1] /= rhs;
            }
            ExprOp::Mod => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let rhs = stack[sp];
                if rhs == 0.0 {
                    return 0.0;
                }
                stack[sp - 1] = fmodf(stack[sp - 1], rhs);
            }
            ExprOp::Neg => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = -stack[sp - 1];
            }
            ExprOp::CallAbs => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = stack[sp - 1].abs();
            }
            ExprOp::CallMin => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = stack[sp - 1].min(stack[sp]);
            }
            ExprOp::CallMax => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = stack[sp - 1].max(stack[sp]);
            }
            ExprOp::CallSin => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = sinf(stack[sp - 1]);
            }
            ExprOp::CallCos => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = cosf(stack[sp - 1]);
            }
            ExprOp::CallSqrt => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = sqrtf(stack[sp - 1]);
            }
            ExprOp::CallPow => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = powf(stack[sp - 1], stack[sp]);
            }
            ExprOp::CallFmod => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let rhs = stack[sp];
                if rhs == 0.0 {
                    return 0.0;
                }
                stack[sp - 1] = fmodf(stack[sp - 1], rhs);
            }
            ExprOp::CallClamp => {
                if sp < 3 {
                    return 0.0;
                }
                let hi = stack[sp - 1];
                let lo = stack[sp - 2];
                let value = stack[sp - 3];
                sp -= 2;
                stack[sp - 1] = clamp_f32(value, lo, hi);
            }
            ExprOp::CmpEq => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Eq, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpNe => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Ne, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpGt => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Gt, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpLt => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Lt, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpGe => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Ge, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpLe => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Le, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::BoolAnd => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let lhs = stack[sp - 1] != 0.0;
                let rhs = stack[sp] != 0.0;
                stack[sp - 1] = if lhs && rhs { 1.0 } else { 0.0 };
            }
            ExprOp::BoolOr => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let lhs = stack[sp - 1] != 0.0;
                let rhs = stack[sp] != 0.0;
                stack[sp - 1] = if lhs || rhs { 1.0 } else { 0.0 };
            }
            ExprOp::BoolNot => {
                if sp < 1 {
                    return 0.0;
                }
                let value = stack[sp - 1] != 0.0;
                stack[sp - 1] = if value { 0.0 } else { 1.0 };
            }
        }
    }

    if sp == 0 { 0.0 } else { stack[0] }
}

const MAX_PID_LOOPS: usize = 8;
pub const MAX_TRANSITIONS_PER_TASK_PER_TICK: usize = 64;
pub const MAX_ACTIVE_TASKS: usize = 64;
pub const MAX_VARIABLES: usize = 64;
pub const MAX_EXPR_OPS: usize = 32;
pub const MAX_EXPR_STACK: usize = 16;
pub const MAX_CAM_POINTS: usize = 256;
pub const MAX_CAM_COUPLINGS: usize = 8;
pub const MAX_AXIS_HOMING_TARGETS: usize = 32;
pub const MAX_EXTERN_ARGS: usize = 16;
pub const MAX_EXTERN_RETURNS: usize = 8;
pub const MAX_TRACKED_DIGITAL_OUTPUTS: usize = 1024;
pub const MAX_WORKPIECE_TOKENS: usize = 256;
pub const MAX_WORKPIECE_LINEAGE_RECORDS: usize = MAX_WORKPIECE_TOKENS * 4;
pub const SEMANTIC_RESOURCE_CONFLICT_ERROR_CODE: i32 = -32_001;

#[derive(Debug, Clone, Copy, PartialEq)]
struct PidState {
    integral: f32,
    prev_error: f32,
    last_updated: Option<Tick>,
}

impl Default for PidState {
    fn default() -> Self {
        Self {
            integral: 0.0,
            prev_error: 0.0,
            last_updated: None,
        }
    }
}

impl<'a> Runtime<'a> {
    fn update_pid_loops<IO: Io>(&mut self, now: Tick, io: &mut IO) {
        // Keep this branch-free for the common case: no PID loops.
        if self.program.pid_loops.is_empty() {
            return;
        }

        for (idx, cfg) in self.program.pid_loops.iter().enumerate() {
            if idx >= MAX_PID_LOOPS {
                break;
            }
            let state = &mut self.pid_states[idx];
            if !pid_should_run(now, state.last_updated, cfg.period_ticks) {
                continue;
            }
            let out = pid_step(cfg, state, io.read_analog_input(cfg.pv));
            io.write_analog_output(cfg.out, out);
            state.last_updated = Some(now);
        }
    }

    fn update_cam_couplings<IO: Io>(&mut self, _now: Tick, io: &mut IO) {
        if self.program.cam_configs.is_empty() {
            return;
        }

        for (idx, cfg) in self.program.cam_configs.iter().enumerate() {
            if idx >= MAX_CAM_COUPLINGS {
                break;
            }

            let state = &mut self.cam_states[idx];
            if !state.engaged {
                continue;
            }

            let Some(table) = self.program.cam_tables.get(state.active_table as usize) else {
                state.fault = true;
                state.engaged = false;
                state.in_sync = false;
                continue;
            };

            state.master_pos = io.read_analog_input(cfg.master_input);
            let adjusted_master = state.master_pos * cfg.gear_ratio + state.phase_offset;
            state.slave_cmd = interpolate_cam(cfg.interpolation, table, adjusted_master);

            if state.switch_decay_ticks > 0 {
                state.slave_cmd += state.switch_offset;
                state.switch_offset *= 0.95;
                state.switch_decay_ticks -= 1;
            }

            io.write_analog_output(cfg.slave_output, state.slave_cmd);

            state.slave_actual = io.read_analog_input(cfg.slave_feedback);
            state.following_error = (state.slave_cmd - state.slave_actual).abs();

            let limit = cfg.following_error_limit;
            state.in_sync = limit > 0.0 && state.following_error < limit;
            if limit > 0.0 && state.following_error > limit * 3.0 {
                state.fault = true;
                state.engaged = false;
                state.in_sync = false;
            }
        }
    }
}

fn pid_should_run(now: Tick, last: Option<Tick>, period_ticks: u64) -> bool {
    if period_ticks == 0 {
        return false;
    }
    match last {
        None => true,
        Some(t) => now.0.saturating_sub(t.0) >= period_ticks,
    }
}

fn pid_step(cfg: &PidConfig, state: &mut PidState, pv: f32) -> f32 {
    let sp = cfg.sp;
    let error = sp - pv;

    // Defensive: keep dt strictly positive to avoid NaN in derivative.
    let dt = if cfg.dt_s > 0.0 { cfg.dt_s } else { 1e-6 };

    let derivative = (error - state.prev_error) / dt;

    // Candidate integral update.
    let integral_candidate = state.integral + error * dt;
    let mut u_unsat = cfg.kp * error + cfg.ki * integral_candidate + cfg.kd * derivative;
    // Anti-windup: conditionally accept the integrator update.
    let integral = match cfg.anti_windup {
        AntiWindup::ConditionalIntegration => {
            if u_unsat > cfg.limit_max && error > 0.0 {
                state.integral
            } else if u_unsat < cfg.limit_min && error < 0.0 {
                state.integral
            } else {
                integral_candidate
            }
        }
    };

    u_unsat = cfg.kp * error + cfg.ki * integral + cfg.kd * derivative;
    let out = clamp_f32(u_unsat, cfg.limit_min, cfg.limit_max);

    state.integral = integral;
    state.prev_error = error;

    out
}

fn clamp_f32(v: f32, min: f32, max: f32) -> f32 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

fn analog_in_selected_ranges(value: f32, ranges: &[AnalogRange]) -> bool {
    ranges.iter().any(|r| value >= r.min && value <= r.max)
}

pub fn binary_search_interval(table: &CamTableData, x: f32) -> u16 {
    let n = table.num_points as usize;
    if n < 2 {
        return 0;
    }
    let mut lo = 0usize;
    let mut hi = n - 1;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if table.master[mid] <= x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo as u16
}

pub fn normalize_master(table: &CamTableData, master_pos: f32) -> f32 {
    let n = table.num_points as usize;
    if n == 0 {
        return 0.0;
    }
    let x0 = table.master[0];
    if n == 1 {
        return x0;
    }
    let xn = table.master[n - 1];

    if table.periodic {
        let period = xn - x0;
        if period <= 0.0 {
            return x0;
        }
        let offset = master_pos - x0;
        x0 + offset - floorf(offset / period) * period
    } else if master_pos < x0 {
        x0
    } else if master_pos > xn {
        xn
    } else {
        master_pos
    }
}

pub fn linear_interpolate(table: &CamTableData, master_pos: f32) -> f32 {
    let n = table.num_points as usize;
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return table.slave[0];
    }

    let x = normalize_master(table, master_pos);
    let i = binary_search_interval(table, x) as usize;
    let x0 = table.master[i];
    let x1 = table.master[i + 1];
    let y0 = table.slave[i];
    let y1 = table.slave[i + 1];
    let dx = x1 - x0;
    if dx == 0.0 {
        return y0;
    }
    let t = (x - x0) / dx;
    y0 + t * (y1 - y0)
}

pub fn cubic_interpolate(table: &CamTableData, master_pos: f32) -> f32 {
    let n = table.num_points as usize;
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return table.slave[0];
    }

    let x = normalize_master(table, master_pos);
    let i = binary_search_interval(table, x) as usize;
    let dx = x - table.master[i];
    let c = table.coeffs[i];
    c.a + dx * (c.b + dx * (c.c + dx * c.d))
}

pub fn cubic_derivative(table: &CamTableData, master_pos: f32) -> f32 {
    let n = table.num_points as usize;
    if n < 2 {
        return 0.0;
    }
    let x = normalize_master(table, master_pos);
    let i = binary_search_interval(table, x) as usize;
    let dx = x - table.master[i];
    let c = table.coeffs[i];
    c.b + dx * (2.0 * c.c + 3.0 * c.d * dx)
}

fn interpolate_cam(interpolation: CamInterpolation, table: &CamTableData, master_pos: f32) -> f32 {
    match interpolation {
        CamInterpolation::Linear => linear_interpolate(table, master_pos),
        CamInterpolation::CubicSpline => cubic_interpolate(table, master_pos),
    }
}

fn compare_f32(left: f32, op: CompareOp, right: f32) -> bool {
    match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Gt => left > right,
        CompareOp::Lt => left < right,
        CompareOp::Ge => left >= right,
        CompareOp::Le => left <= right,
    }
}

