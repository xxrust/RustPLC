//! 形式化验证能力测试套件
//!
//! 5 个从简单到困难的测试用例，覆盖 Safety / Liveness / Timing / Causality 四项验证的真实能力。
//! 每个用例构造一个完整的 .plc 程序，走完 parse → semantic → verify 全流水线，
//! 断言验证引擎能正确通过或拒绝，并检查诊断信息的关键内容。

use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
};
use rust_plc::verification::verify_all;

/// 辅助函数：走完全流水线，返回 Ok(验证摘要 JSON) 或 Err(所有诊断文本)
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

fn assert_all_errors_have_location_and_suggestion(errors: &[String]) {
    assert!(!errors.is_empty(), "错误列表不应为空");
    for error in errors {
        assert!(error.contains("位置:"), "每个错误都应包含位置信息: {error}");
        assert!(error.contains("建议:"), "每个错误都应包含修复建议: {error}");
    }
}

// ============================================================================
// 测试 1（简单）：单气缸往返 — 全部验证通过的基线
// ============================================================================
//
// 场景：一个气缸伸出再缩回，有完整的拓扑、因果链、timeout、on_complete 循环。
// 预期：Safety（无冲突约束，完备证明）、Liveness（有 timeout + allow_indefinite_wait）、
//       Timing（stroke_time 220ms < 500ms）、Causality（Y0→valve→cyl→sensor 连通）全部通过。
// 验证能力：确认引擎在"一切正确"时不会误报。

#[test]
fn test1_single_cylinder_all_pass() {
    let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input

device start_button: sensor {
    reports_to: X2
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
    reports_to: X0
    detects: cyl_A.extended
}

device sensor_A_ret: sensor {
    reports_to: X1
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

    let result = full_pipeline(source).expect("单气缸往返应全部验证通过");

    // Safety 应为完备证明（无 conflicts_with 约束时直接 Complete）
    let safety_level = result["safety"]["level"].as_str().unwrap();
    assert!(
        safety_level == "完备证明",
        "无冲突约束时 safety 应为完备证明，实际: {safety_level}"
    );

    // Liveness / Timing / Causality 均通过
    assert_eq!(result["liveness"]["level"], "通过");
    assert_eq!(result["timing"]["level"], "通过");
    assert_eq!(result["causality"]["level"], "通过");
}

// ============================================================================
// 测试 2（中等）：双气缸顺序安全 vs 并行冲突 — Safety 正反对比
// ============================================================================
//
// 场景 A：两个气缸顺序动作（先 A 后 B），有 conflicts_with 约束 → 应通过
// 场景 B：两个气缸并行伸出，同一 conflicts_with 约束 → 应检出冲突
// 验证能力：Safety 引擎能区分"顺序安全"和"并行冲突"，并给出反例路径。

#[test]
fn test2a_sequential_cylinders_safety_pass() {
    let source = r#"
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

    let result = full_pipeline(source).expect("顺序双气缸不应违反 conflicts_with");
    let safety_level = result["safety"]["level"].as_str().unwrap();
    assert!(
        safety_level == "完备证明" || safety_level == "有界验证",
        "顺序逻辑应通过 safety，实际: {safety_level}"
    );
}

#[test]
fn test2b_parallel_cylinders_safety_fail() {
    let source = r#"
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

    let errors = full_pipeline(source).expect_err("并行伸出冲突气缸应触发 safety 错误");
    let joined = errors.join("\n");

    assert!(joined.contains("ERROR [safety]"), "应报告 safety 错误");
    assert!(joined.contains("conflicts_with"), "错误应包含冲突约束名称");
    assert!(joined.contains("位置:"), "错误应包含源码位置");
    assert!(joined.contains("建议:"), "错误应包含修复建议");

    assert_all_errors_have_location_and_suggestion(&errors);

    let safety_count = errors
        .iter()
        .filter(|e| e.contains("ERROR [safety]"))
        .count();
    assert!(
        safety_count >= 1,
        "并行冲突场景应至少产生 1 个 safety 错误，实际: {safety_count}"
    );
}

// ============================================================================
// 测试 3（中等）：Liveness 多场景 — wait 无 timeout / unreachable 无效 / SCC 无出口
// ============================================================================
//
// 场景：一个程序同时触发三种 Liveness 违规：
//   1. wait 无 timeout 且无 allow_indefinite_wait
//   2. on_complete: unreachable 但存在非跳转路径
//   3. 两个 task 互相 goto 形成 SCC，但无 timeout/allow 出边
// 验证能力：Liveness 引擎能同时检出多种死锁风险，不会只报第一个就停。

#[test]
fn test3_liveness_triple_violation() {
    let source = r#"
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

    let errors = full_pipeline(source).expect_err("三重 liveness 违规应被检出");
    let joined = errors.join("\n");

    // 违规 1：wait 无 timeout
    assert!(
        joined.contains("缺少 timeout 分支"),
        "应检出 wait 无 timeout 的死锁风险"
    );

    // 违规 2：SCC 无 timeout/allow 出边（spin_a ↔ spin_b）
    assert!(
        joined.contains("强连通分量"),
        "应检出 SCC 无超时出口的死锁风险"
    );

    // 违规 3：on_complete: unreachable 标记无效
    assert!(
        joined.contains("on_complete: unreachable"),
        "应检出 unreachable 标记与控制流不一致"
    );

    assert_all_errors_have_location_and_suggestion(&errors);

    // 至少报告 3 个 liveness 问题（对应三类违规）
    let liveness_count = errors
        .iter()
        .filter(|e| e.contains("ERROR [liveness]"))
        .count();
    assert!(
        liveness_count >= 3,
        "应同时报告三类 liveness 问题，实际只报了 {liveness_count} 个"
    );
}

// ============================================================================
// 测试 4（较难）：Timing + Causality 联合检测 — 时序超限 + 因果链断裂
// ============================================================================
//
// 场景：
//   - 气缸 stroke_time=200ms + 上游 valve response_time=20ms = 220ms，
//     但 must_complete_within 100ms → Timing 违反
//   - 因果链声明 Y0 → valve_A → cyl_A → sensor_A_ext，
//     但 cyl_A 缺少 driven_by: valve_A → Causality 断裂
//   - must_start_after 200ms 但前驱 timeout 只有 50ms → Timing must_start_after 违反
// 验证能力：Timing 和 Causality 引擎能同时工作，各自独立报告问题，
//           且 Timing 能正确计算上游 response_time 链路时间。

#[test]
fn test4_timing_and_causality_combined_failure() {
    let source = r#"
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
    reports_to: X0
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

    let errors = full_pipeline(source).expect_err("timing + causality 联合违规应被检出");
    let joined = errors.join("\n");

    // Timing must_complete_within 违反
    assert!(joined.contains("ERROR [timing]"), "应报告 timing 错误");
    assert!(
        joined.contains("无法在 100ms 内完成") || joined.contains("must_complete_within"),
        "timing 错误应指出超限"
    );

    // Causality 断裂（cyl_A 缺少 driven_by: valve_A）
    assert!(
        joined.contains("ERROR [causality]"),
        "应报告 causality 错误"
    );
    assert!(
        joined.contains("valve_A") && joined.contains("cyl_A"),
        "causality 错误应指出 valve_A → cyl_A 断裂"
    );

    // must_start_after 违反（前驱 timeout 50ms < 要求 200ms）
    assert!(
        joined.contains("无法保证") && joined.contains("200ms"),
        "应报告 must_start_after 违反"
    );

    let timing_count = errors
        .iter()
        .filter(|e| e.contains("ERROR [timing]"))
        .count();
    let causality_count = errors
        .iter()
        .filter(|e| e.contains("ERROR [causality]"))
        .count();
    assert!(
        timing_count >= 2,
        "应至少报告 2 个 timing 错误（complete_within + must_start_after），实际 {timing_count}"
    );
    assert!(
        causality_count >= 1,
        "应至少报告 1 个 causality 错误，实际 {causality_count}"
    );

    assert_all_errors_have_location_and_suggestion(&errors);
}

// ============================================================================
// 测试 5（困难）：四项验证全部失败的复杂工业场景
// ============================================================================
//
// 场景：模拟一个真实的双工位加工站，故意引入四类错误：
//   1. Safety：parallel 块同时伸出两个夹具（conflicts_with 违反）
//   2. Liveness：error_recovery task 中 wait 无 timeout 且无 allow_indefinite_wait
//   3. Timing：task.main.clamp_both 的 must_complete_within 50ms，
//      但夹具 stroke_time=300ms + 上游 response_time=25ms = 325ms
//   4. Causality：声明 Y2 → valve_C → clamp_B → sensor_B_clamped，
//      但 clamp_B 缺少 driven_by: valve_C
//
// 验证能力：在一个接近真实复杂度的程序上，四项验证引擎全部独立工作，
//           同时报告所有问题，错误信息包含行号、原因和修复建议。

#[test]
fn test5_industrial_dual_station_all_four_failures() {
    let source = r#"
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
    reports_to: X0
    detects: clamp_A.extended
}

device sensor_A_released: sensor {
    reports_to: X1
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
    reports_to: X2
    detects: clamp_B.extended
}

device sensor_B_released: sensor {
    reports_to: X3
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

    let errors = full_pipeline(source).expect_err("四项验证应全部失败");
    let joined = errors.join("\n\n");

    // 1. Safety 违反
    assert!(
        joined.contains("ERROR [safety]"),
        "应报告 safety 错误（race 分支可能导致两夹具同时激活）"
    );

    // 2. Liveness 违反
    assert!(
        joined.contains("ERROR [liveness]"),
        "应报告 liveness 错误（error_recovery 的 wait 无 timeout）"
    );

    // 3. Timing 违反
    assert!(
        joined.contains("ERROR [timing]"),
        "应报告 timing 错误（stroke_time 325ms > 约束 50ms）"
    );

    // 4. Causality 违反
    assert!(
        joined.contains("ERROR [causality]"),
        "应报告 causality 错误（clamp_B 缺少 driven_by: valve_C）"
    );

    assert_all_errors_have_location_and_suggestion(&errors);

    // 至少 4 个不同的错误（每个 checker 至少 1 个）
    let unique_checkers: std::collections::HashSet<&str> = errors
        .iter()
        .filter_map(|e| {
            if e.contains("ERROR [safety]") {
                Some("safety")
            } else if e.contains("ERROR [liveness]") {
                Some("liveness")
            } else if e.contains("ERROR [timing]") {
                Some("timing")
            } else if e.contains("ERROR [causality]") {
                Some("causality")
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        unique_checkers.len(),
        4,
        "应同时触发全部四个 checker 的错误，实际只触发了: {:?}",
        unique_checkers
    );
}

#[test]
fn test_analog_safety_constraint_parses_and_builds_ir() {
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::{build_constraint_set, preprocess_program};

    let source = r#"
[topology]
device pressure_sensor: analog_input { range: 0..100, unit: "bar" }
device heater: digital_output

[constraints]
safety: pressure_sensor > 80 conflicts_with heater.on
    reason: "High pressure conflicts with heater operation"

[tasks]
task main:
    step s1:
        action: log "test"
"#;

    // Parse the source
    let program = parse_plc(source).expect("should parse");

    // Preprocess (expand repeat/delay)
    let expanded = preprocess_program(&program).expect("should preprocess");

    // Build constraint set
    let constraints = build_constraint_set(&expanded).expect("should build constraints");

    // Verify the safety constraint was built
    assert_eq!(
        constraints.safety.len(),
        1,
        "should have one safety constraint"
    );

    let rule = &constraints.safety[0];

    // Check left operand is Threshold
    match &rule.left {
        rust_plc::ir::SafetyExpr::Threshold {
            device,
            operator,
            value,
        } => {
            assert_eq!(device, "pressure_sensor");
            assert_eq!(operator, ">");
            assert_eq!(value, "80");
        }
        _ => panic!("left operand should be Threshold"),
    }

    // Check right operand is State
    match &rule.right {
        rust_plc::ir::SafetyExpr::State(state_expr) => {
            assert_eq!(state_expr.device, "heater");
            assert_eq!(state_expr.state, "on");
        }
        _ => panic!("right operand should be State"),
    }

    // Check relation
    assert_eq!(rule.relation, rust_plc::ir::SafetyRelation::ConflictsWith);
}

#[test]
fn test_analog_safety_constraint_with_requires_relation() {
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::{build_constraint_set, preprocess_program};

    let source = r#"
[topology]
device temperature: analog_input { range: 0..200, unit: "C" }
device pump: digital_output

[constraints]
safety: pump.on requires temperature < 150
    reason: "Pump requires temperature below 150C"

[tasks]
task main:
    step s1:
        action: log "test"
"#;

    let program = parse_plc(source).expect("should parse");
    let expanded = preprocess_program(&program).expect("should preprocess");
    let constraints = build_constraint_set(&expanded).expect("should build constraints");

    assert_eq!(constraints.safety.len(), 1);
    let rule = &constraints.safety[0];

    // Check left operand is State
    match &rule.left {
        rust_plc::ir::SafetyExpr::State(state_expr) => {
            assert_eq!(state_expr.device, "pump");
            assert_eq!(state_expr.state, "on");
        }
        _ => panic!("left operand should be State"),
    }

    // Check right operand is Threshold
    match &rule.right {
        rust_plc::ir::SafetyExpr::Threshold {
            device,
            operator,
            value,
        } => {
            assert_eq!(device, "temperature");
            assert_eq!(operator, "<");
            assert_eq!(value, "150");
        }
        _ => panic!("right operand should be Threshold"),
    }

    assert_eq!(rule.relation, rust_plc::ir::SafetyRelation::Requires);
}

#[test]
fn test_analog_safety_constraint_both_sides_threshold() {
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::{build_constraint_set, preprocess_program};

    let source = r#"
[topology]
device pressure1: analog_input { range: 0..100, unit: "bar" }
device pressure2: analog_input { range: 0..100, unit: "bar" }

[constraints]
safety: pressure1 > 80 conflicts_with pressure2 > 80
    reason: "Both pressures cannot exceed 80 simultaneously"

[tasks]
task main:
    step s1:
        action: log "test"
"#;

    let program = parse_plc(source).expect("should parse");
    let expanded = preprocess_program(&program).expect("should preprocess");
    let constraints = build_constraint_set(&expanded).expect("should build constraints");

    assert_eq!(constraints.safety.len(), 1);
    let rule = &constraints.safety[0];

    // Both sides should be Threshold
    match &rule.left {
        rust_plc::ir::SafetyExpr::Threshold {
            device,
            operator,
            value,
        } => {
            assert_eq!(device, "pressure1");
            assert_eq!(operator, ">");
            assert_eq!(value, "80");
        }
        _ => panic!("left operand should be Threshold"),
    }

    match &rule.right {
        rust_plc::ir::SafetyExpr::Threshold {
            device,
            operator,
            value,
        } => {
            assert_eq!(device, "pressure2");
            assert_eq!(operator, ">");
            assert_eq!(value, "80");
        }
        _ => panic!("right operand should be Threshold"),
    }
}

#[test]
fn test_analog_safety_constraint_all_comparison_operators() {
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::{build_constraint_set, preprocess_program};

    let operators = vec![
        ("pressure > 80", ">"),
        ("pressure < 20", "<"),
        ("pressure >= 75", ">="),
        ("pressure <= 25", "<="),
        ("pressure == 50", "=="),
        ("pressure != 50", "!="),
    ];

    for (condition, expected_op) in operators {
        let source = format!(
            r#"
[topology]
device pressure: analog_input {{ range: 0..100, unit: "bar" }}
device heater: digital_output

[constraints]
safety: {condition} conflicts_with heater.on

[tasks]
task main:
    step s1:
        action: log "test"
"#
        );

        let program = parse_plc(&source).expect(&format!("should parse {}", condition));
        let expanded = preprocess_program(&program).expect("should preprocess");
        let constraints = build_constraint_set(&expanded).expect("should build constraints");

        assert_eq!(constraints.safety.len(), 1);
        let rule = &constraints.safety[0];

        match &rule.left {
            rust_plc::ir::SafetyExpr::Threshold { operator, .. } => {
                assert_eq!(operator, expected_op, "operator mismatch for {}", condition);
            }
            _ => panic!("left operand should be Threshold for {}", condition),
        }
    }
}

#[test]
fn test_analog_safety_constraint_produces_correct_ir_json() {
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::{build_constraint_set, preprocess_program};

    let source = r#"
[topology]
device pressure_sensor: analog_input { range: 0..100, unit: "bar" }
device heater: digital_output

[constraints]
safety: pressure_sensor > 80 conflicts_with heater.on
    reason: "High pressure conflicts with heater operation"

[tasks]
task main:
    step s1:
        action: log "test"
"#;

    let program = parse_plc(source).expect("should parse");
    let expanded = preprocess_program(&program).expect("should preprocess");
    let constraints = build_constraint_set(&expanded).expect("should build constraints");

    // Serialize to JSON to verify structure
    let json = serde_json::to_value(&constraints).expect("should serialize to JSON");

    assert!(json["safety"].is_array());
    assert_eq!(json["safety"].as_array().unwrap().len(), 1);

    let safety_rule = &json["safety"][0];

    // Verify left side is Threshold
    assert_eq!(safety_rule["left"]["kind"], "threshold");
    assert_eq!(safety_rule["left"]["device"], "pressure_sensor");
    assert_eq!(safety_rule["left"]["operator"], ">");
    assert_eq!(safety_rule["left"]["value"], "80");

    // Verify right side is State
    assert_eq!(safety_rule["right"]["kind"], "state");
    assert_eq!(safety_rule["right"]["device"], "heater");
    assert_eq!(safety_rule["right"]["state"], "on");

    // Verify relation
    assert_eq!(safety_rule["relation"], "conflicts_with");

    // Verify reason
    assert_eq!(
        safety_rule["reason"],
        "High pressure conflicts with heater operation"
    );
}

#[test]
fn test_analog_safety_constraint_mixed_operands_in_json() {
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::{build_constraint_set, preprocess_program};

    let source = r#"
[topology]
device temp: analog_input { range: 0..200, unit: "C" }
device pump: digital_output

[constraints]
safety: pump.on requires temp < 150

[tasks]
task main:
    step s1:
        action: log "test"
"#;

    let program = parse_plc(source).expect("should parse");
    let expanded = preprocess_program(&program).expect("should preprocess");
    let constraints = build_constraint_set(&expanded).expect("should build constraints");

    let json = serde_json::to_value(&constraints).expect("should serialize to JSON");
    let safety_rule = &json["safety"][0];

    // Left: State
    assert_eq!(safety_rule["left"]["kind"], "state");
    assert_eq!(safety_rule["left"]["device"], "pump");
    assert_eq!(safety_rule["left"]["state"], "on");

    // Right: Threshold
    assert_eq!(safety_rule["right"]["kind"], "threshold");
    assert_eq!(safety_rule["right"]["device"], "temp");
    assert_eq!(safety_rule["right"]["operator"], "<");
    assert_eq!(safety_rule["right"]["value"], "150");

    assert_eq!(safety_rule["relation"], "requires");
}

#[test]
fn test_analog_safety_constraint_validation_checks_device_exists() {
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::{build_constraint_set, preprocess_program};

    let source = r#"
[topology]
device heater: digital_output

[constraints]
safety: undefined_sensor > 80 conflicts_with heater.on

[tasks]
task main:
    step s1:
        action: log "test"
"#;

    let program = parse_plc(source).expect("should parse");
    let expanded = preprocess_program(&program).expect("should preprocess");
    let result = build_constraint_set(&expanded);

    // Should fail because undefined_sensor doesn't exist
    assert!(
        result.is_err(),
        "should fail validation for undefined device"
    );
    let errors = result.unwrap_err();
    assert!(!errors.is_empty());
    let error_msg = errors[0].to_string();
    assert!(
        error_msg.contains("undefined_sensor") || error_msg.contains("未定义"),
        "error should mention undefined device: {}",
        error_msg
    );
}

#[test]
fn test_analog_safety_constraint_parser_handles_decimal_values() {
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::{build_constraint_set, preprocess_program};

    let source = r#"
[topology]
device voltage: analog_input { range: 0..24, unit: "V" }
device relay: digital_output

[constraints]
safety: voltage > 12.5 conflicts_with relay.on

[tasks]
task main:
    step s1:
        action: log "test"
"#;

    let program = parse_plc(source).expect("should parse");
    let expanded = preprocess_program(&program).expect("should preprocess");
    let constraints = build_constraint_set(&expanded).expect("should build constraints");

    let rule = &constraints.safety[0];
    match &rule.left {
        rust_plc::ir::SafetyExpr::Threshold { value, .. } => {
            assert_eq!(value, "12.5", "should preserve decimal value");
        }
        _ => panic!("should be Threshold"),
    }
}

#[test]
fn test_analog_safety_constraint_parser_handles_large_numbers() {
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::{build_constraint_set, preprocess_program};

    let source = r#"
[topology]
device counter: analog_input { range: 0..10000, unit: "count" }
device alarm: digital_output

[constraints]
safety: counter > 9999 conflicts_with alarm.on

[tasks]
task main:
    step s1:
        action: log "test"
"#;

    let program = parse_plc(source).expect("should parse");
    let expanded = preprocess_program(&program).expect("should preprocess");
    let constraints = build_constraint_set(&expanded).expect("should build constraints");

    let rule = &constraints.safety[0];
    match &rule.left {
        rust_plc::ir::SafetyExpr::Threshold { value, .. } => {
            assert_eq!(value, "9999");
        }
        _ => panic!("should be Threshold"),
    }
}
