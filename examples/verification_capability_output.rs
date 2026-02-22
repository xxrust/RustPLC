use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
};
use rust_plc::verification::verify_all;

fn full_pipeline(source: &str) -> Result<serde_json::Value, Vec<String>> {
    let program = parse_plc(source).map_err(|e| vec![e.to_string()])?;

    let topology = build_topology_graph(&program)
        .map_err(|errs| errs.into_iter().map(|e| e.to_string()).collect::<Vec<_>>())?;
    let state_machine = build_state_machine(&program)
        .map_err(|errs| errs.into_iter().map(|e| e.to_string()).collect::<Vec<_>>())?;
    let constraints = build_constraint_set(&program)
        .map_err(|errs| errs.into_iter().map(|e| e.to_string()).collect::<Vec<_>>())?;
    let _timing_model = build_timing_model(&program)
        .map_err(|errs| errs.into_iter().map(|e| e.to_string()).collect::<Vec<_>>())?;

    let summary =
        verify_all(&program, &topology, &constraints, &state_machine).map_err(|issues| {
            issues
                .into_iter()
                .map(|issue| issue.to_string())
                .collect::<Vec<_>>()
        })?;

    serde_json::to_value(&summary).map_err(|e| vec![e.to_string()])
}

fn run_case(name: &str, source: &str) {
    println!("== CASE: {name} ==");
    match full_pipeline(source) {
        Ok(summary) => {
            println!("RESULT: PASS");
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".to_string())
            );
        }
        Err(errors) => {
            println!("RESULT: FAIL");
            for (idx, error) in errors.iter().enumerate() {
                println!("--- error {} ---", idx + 1);
                println!("{error}");
            }
        }
    }
    println!();
}

fn main() {
    let test1 = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input

device start_button: digital_input {
    driven_by: X2
    debounce: 20ms
}

device valve_A: solenoid_valve {
    driven_by: Y0
    response_time: 20ms
}

device cyl_A: cylinder {
    driven_by: valve_A
    stroke_time: 200ms
    retract_time: 180ms
}

device sensor_A_ext: sensor {
    driven_by: X0
    detects: cyl_A.extended
}

device sensor_A_ret: sensor {
    driven_by: X1
    detects: cyl_A.retracted
}

[constraints]

timing: task.work.step_extend must_complete_within 500ms
    reason: "单步伸出不应超过500ms"

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
    reason: "Y0 驱动 valve_A 推动 cyl_A 由 sensor_A_ext 检测"

[tasks]

task work:
    step step_extend:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 400ms -> goto fault

    step step_retract:
        action: retract cyl_A
        wait: sensor_A_ret == true
        timeout: 400ms -> goto fault

    on_complete: goto ready

task fault:
    step safe:
        action: retract cyl_A
    step alarm:
        action: log "动作超时"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto work
"#;

    let test2a = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_A: solenoid_valve { driven_by: Y0, response_time: 15ms }
device valve_B: solenoid_valve { driven_by: Y1, response_time: 15ms }

device cyl_A: cylinder { driven_by: valve_A, stroke_time: 200ms, retract_time: 180ms }
device cyl_B: cylinder { driven_by: valve_B, stroke_time: 250ms, retract_time: 220ms }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸同时伸出会导致机械碰撞"

[tasks]

task seq:
    step extend_A:
        action: extend cyl_A
    step retract_A:
        action: retract cyl_A
    step extend_B:
        action: extend cyl_B
    step retract_B:
        action: retract cyl_B
"#;

    let test2b = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_A: solenoid_valve { driven_by: Y0, response_time: 15ms }
device valve_B: solenoid_valve { driven_by: Y1, response_time: 15ms }

device cyl_A: cylinder { driven_by: valve_A, stroke_time: 200ms, retract_time: 180ms }
device cyl_B: cylinder { driven_by: valve_B, stroke_time: 250ms, retract_time: 220ms }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸同时伸出会导致机械碰撞"

[tasks]

task par:
    step move_together:
        parallel:
            branch_A:
                action: extend cyl_A
            branch_B:
                action: extend cyl_B
"#;

    let test3 = r#"
[topology]

[constraints]

[tasks]

task entry:
    step bare_wait:
        wait: sensor_X == true
    on_complete: unreachable

task spin_a:
    step do_a:
        action: log "a"
    on_complete: goto spin_b

task spin_b:
    step do_b:
        action: log "b"
    on_complete: goto spin_a
"#;

    let test4 = r#"
[topology]

device Y0: digital_output
device X0: digital_input

device valve_A: solenoid_valve {
    driven_by: Y0
    response_time: 20ms
}

device cyl_A: cylinder {
    stroke_time: 200ms
    retract_time: 180ms
}

device sensor_A_ext: sensor {
    driven_by: X0
    detects: cyl_A.extended
}

[constraints]

timing: task.work.do_extend must_complete_within 100ms
    reason: "单步不应超过100ms"

timing: task.cooldown must_start_after 200ms
    reason: "冷却前需要等待泄压"

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
    reason: "Y0 驱动 valve_A 推动 cyl_A 由 sensor_A_ext 检测"

[tasks]

task work:
    step do_extend:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 50ms -> goto cooldown

task cooldown:
    step begin:
        action: log "冷却中"
"#;

    let test5 = r#"
[topology]

# ===== 控制器端口 =====
device Y0: digital_output
device Y2: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input
device X3: digital_input

# ===== 工位 A 夹具 =====
device valve_A: solenoid_valve {
    driven_by: Y0
    response_time: 25ms
}

device clamp_A: cylinder {
    driven_by: valve_A
    stroke_time: 300ms
    retract_time: 280ms
}

device sensor_A_clamped: sensor {
    driven_by: X0
    detects: clamp_A.extended
}

device sensor_A_released: sensor {
    driven_by: X1
    detects: clamp_A.retracted
}

# ===== 工位 B 夹具 =====
device valve_C: solenoid_valve {
    driven_by: Y2
    response_time: 25ms
}

# 故意缺少 driven_by: valve_C → 触发 Causality 断裂
device clamp_B: cylinder {
    stroke_time: 300ms
    retract_time: 280ms
}

device sensor_B_clamped: sensor {
    driven_by: X2
    detects: clamp_B.extended
}

device sensor_B_released: sensor {
    driven_by: X3
    detects: clamp_B.retracted
}

[constraints]

# Safety：两工位夹具不能同时夹紧
safety: clamp_A.extended conflicts_with clamp_B.extended
    reason: "共用气源，两工位同时夹紧会导致压力不足"

# Timing：故意设置过紧的时序约束
timing: task.main.clamp_both must_complete_within 50ms
    reason: "夹紧动作不应超过50ms"

# Causality：声明完整链路，但 clamp_B 缺少 connected_to
causality: Y0 -> valve_A -> clamp_A -> sensor_A_clamped
    reason: "Y0 驱动 valve_A 推动 clamp_A 由 sensor_A_clamped 检测"
causality: Y2 -> valve_C -> clamp_B -> sensor_B_clamped
    reason: "Y2 驱动 valve_C 推动 clamp_B 由 sensor_B_clamped 检测"

[tasks]

# parallel 块同时伸出两个夹具 → Safety 违反
task main:
    step clamp_both:
        parallel:
            branch_A:
                action: extend clamp_A
            branch_B:
                action: extend clamp_B
    step process:
        action: log "加工中"
    step release:
        action: retract clamp_A
        action: retract clamp_B
    on_complete: goto error_recovery

# Liveness 违反：wait 无 timeout 且无 allow_indefinite_wait
task error_recovery:
    step wait_manual_reset:
        action: retract clamp_A
        action: retract clamp_B
        wait: sensor_A_released == true
    step resume:
        action: log "手动复位完成"
    on_complete: goto main
"#;

    run_case("测试1：单气缸往返（全通过）", test1);
    run_case("测试2a：双气缸顺序（Safety 通过）", test2a);
    run_case("测试2b：双气缸并行（Safety 失败）", test2b);
    run_case("测试3：Liveness 三重违规", test3);
    run_case("测试4：Timing + Causality 联合违规", test4);
    run_case("测试5：工业双工位四类违规", test5);
}
