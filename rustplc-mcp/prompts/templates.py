"""
Prompts: common scenario templates
"""

from mcp.server.fastmcp import FastMCP


def register_prompt_templates(mcp: FastMCP):

    @mcp.prompt()
    def generate_plc_from_description(description: str) -> str:
        """
        从自然语言工艺描述生成 RustPLC DSL 程序。
        按照 SKILL.md 的四阶段流程执行，多轮确认后生成可验证的 .plc 文件。

        Args:
            description: 工艺描述，例如"推料缸把工件推到位，传感器检测到后压紧缸下压"
        """
        return (
            "请按照 RustPLC DSL 生成流程，帮我生成一个 .plc 文件。\n\n"
            f"工艺描述：\n{description}\n\n"
            "请先调用 get_plc_generation_guide 工具获取完整的生成指引，然后严格按照四阶段流程执行：\n"
            "1. 阶段零：生成 .system.md 并等待我确认\n"
            "2. 阶段一：理解工艺，整理动作时序表并等待我确认\n"
            "3. 阶段二：推理设备拓扑并等待我确认\n"
            "4. 阶段三：推导约束并等待我确认\n"
            "5. 阶段四：生成 DSL，调用 validate_plc 验证，修复直到通过\n\n"
            "不要跳过任何阶段，不要凭空假设，遇到不确定的地方必须提问。"
        )

    @mcp.prompt()
    def two_cylinder_template(start_mode: str = "button", cycle_mode: str = "single") -> str:
        """
        双气缸顺序动作 .plc 模板。

        Args:
            start_mode: 启动方式，"button"（按钮）或 "signal"（外部信号）
            cycle_mode: 循环模式，"single"（单次）或 "auto"（自动循环）
        """
        cycle_goto = "ready" if cycle_mode == "single" else "cycle"

        if start_mode == "button":
            start_device_decl = (
                'device start_button: sensor {\n'
                '    purpose: "操作员启动按钮",\n'
                '    subtype: "push_button",\n'
                '    debounce: 20ms\n'
                '}'
            )
            start_relation = "relation { from: start_button.out, to: plc_main.X4, via: reports_to }"
            start_wait_lines = (
                "        wait: start_button == true\n"
                "        allow_indefinite_wait: true"
            )
        else:
            start_device_decl = (
                'device start_signal: digital_input {\n'
                '    purpose: "外部启动信号",\n'
                '    debounce: 20ms\n'
                '}'
            )
            start_relation = ""
            start_wait_lines = (
                "        wait: start_signal == true\n"
                "        timeout: 5000ms -> goto fault_handler"
            )

        lines = [
            f"以下是双气缸顺序动作的 .plc 模板（启动方式: {start_mode}，循环模式: {cycle_mode}）。",
            "请根据实际工艺调整设备名称、时序参数和安全约束，然后调用 validate_plc 验证。",
            "",
            "```plc",
            "[topology]",
            "",
            "device plc_main: plc {",
            '    purpose: "控制器本体与工艺 I/O 端口映射",',
            "    ports: [Y0:digital:producer, Y1:digital:producer, X0:digital:consumer,"
            " X1:digital:consumer, X2:digital:consumer, X3:digital:consumer, X4:digital:consumer]",
            "}",
            "",
            start_device_decl,
            "",
            "device valve_A: solenoid_valve {",
            '    purpose: "驱动 A 缸伸出/缩回的气动电磁阀",',
            "    response_time: 20ms",
            "}",
            "",
            "device cyl_A: cylinder {",
            '    purpose: "执行顺序动作第一步的气缸",',
            "    stroke_time: 300ms,",
            "    retract_time: 300ms",
            "}",
            "",
            "device sensor_A_ext: sensor {",
            '    purpose: "检测 A 缸已完全伸出到位的限位开关",',
            '    subtype: "limit_switch"',
            "}",
            "",
            "device sensor_A_ret: sensor {",
            '    purpose: "检测 A 缸已完全缩回到位的限位开关",',
            '    subtype: "limit_switch"',
            "}",
            "",
            "device valve_B: solenoid_valve {",
            '    purpose: "驱动 B 缸伸出/缩回的气动电磁阀",',
            "    response_time: 20ms",
            "}",
            "",
            "device cyl_B: cylinder {",
            '    purpose: "执行顺序动作第二步的气缸",',
            "    stroke_time: 300ms,",
            "    retract_time: 300ms",
            "}",
            "",
            "device sensor_B_ext: sensor {",
            '    purpose: "检测 B 缸已完全伸出到位的限位开关",',
            '    subtype: "limit_switch"',
            "}",
            "",
            "device sensor_B_ret: sensor {",
            '    purpose: "检测 B 缸已完全缩回到位的限位开关",',
            '    subtype: "limit_switch"',
            "}",
            "",
        ]

        if start_relation:
            lines.append(start_relation)

        lines += [
            "relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }",
            "relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }",
            "relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }",
            "relation { from: sensor_A_ext.out, to: plc_main.X0, via: reports_to }",
            "relation { from: cyl_A.retracted, to: sensor_A_ret.sense, via: detects }",
            "relation { from: sensor_A_ret.out, to: plc_main.X1, via: reports_to }",
            "relation { from: plc_main.Y1, to: valve_B.coil, via: driven_by }",
            "relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }",
            "relation { from: cyl_B.extended, to: sensor_B_ext.sense, via: detects }",
            "relation { from: sensor_B_ext.out, to: plc_main.X2, via: reports_to }",
            "relation { from: cyl_B.retracted, to: sensor_B_ret.sense, via: detects }",
            "relation { from: sensor_B_ret.out, to: plc_main.X3, via: reports_to }",
            "",
            "[constraints]",
            "",
            "# TODO: 根据实际物理干涉情况添加安全约束",
            "# safety: cyl_A.extended conflicts_with cyl_B.extended",
            "",
            "timing: task.cycle must_complete_within 3000ms",
            "",
            "[tasks]",
            "",
            "task cycle:",
            "    step extend_A:",
            "        action: extend cyl_A",
            "        wait: sensor_A_ext == true",
            "        timeout: 500ms -> goto fault_handler",
            "    step extend_B:",
            "        action: extend cyl_B",
            "        wait: sensor_B_ext == true",
            "        timeout: 500ms -> goto fault_handler",
            "    step retract_B:",
            "        action: retract cyl_B",
            "        wait: sensor_B_ret == true",
            "        timeout: 500ms -> goto fault_handler",
            "    step retract_A:",
            "        action: retract cyl_A",
            "        wait: sensor_A_ret == true",
            "        timeout: 500ms -> goto fault_handler",
            f"    on_complete: goto {cycle_goto}",
            "",
            "task fault_handler:",
            "    step safe:",
            "        action: retract cyl_A",
            "        action: retract cyl_B",
            "    step alarm:",
            '        action: log "动作超时报警，已复位到安全位"',
            "    on_complete: goto ready",
            "",
            "task ready:",
            "    step wait_start:",
            start_wait_lines,
            "    on_complete: goto cycle",
            "```",
        ]

        return "\n".join(lines)

    @mcp.prompt()
    def extern_function_template(
        func_name: str,
        params: str,
        return_type: str,
        rust_module: str,
        pure: str = "true",
        time_bound_us: str = "50",
    ) -> str:
        """
        生成 extern function 声明和调用示例。

        Args:
            func_name: 函数名，如 "quadratic_fit"
            params: 参数列表，如 "x: float, y: float"
            return_type: 返回类型，如 "float" 或 "(float, float, float)"
            rust_module: Rust 模块路径，如 "math::fit"
            pure: 是否纯函数，"true" 或 "false"
            time_bound_us: 时间上界（微秒），如 "50"
        """
        is_tuple = return_type.startswith("(")
        param_names = [p.split(":")[0].strip() for p in params.split(",") if p.strip()]
        args_str = ", ".join(param_names)

        if is_tuple:
            inner = return_type.strip("()").split(",")
            out_vars = ", ".join(f"out_{i}" for i in range(len(inner)))
            binding = f"-> ({out_vars})"
            var_decls = "\n".join(
                f"variable out_{i}: {t.strip()} = 0.0" for i, t in enumerate(inner)
            )
        else:
            binding = "-> result"
            var_decls = f"variable result: {return_type} = 0.0"

        input_var_decls = "\n".join(
            f"variable {p.split(':')[0].strip()}: {p.split(':')[1].strip()} = 0.0"
            for p in params.split(",")
            if ":" in p
        )

        pure_note = (
            "纯函数，验证引擎可做确定性推断"
            if pure == "true"
            else "有状态函数，避免在 parallel/race 分支中并发调用"
        )

        lines = [
            f"以下是 `{func_name}` 的 extern function 声明和调用示例。",
            "请将声明放入 `[topology]`，调用放入对应的 task step 中。",
            "",
            "```plc",
            "[topology]",
            "",
            "# 输入变量",
            input_var_decls,
            "",
            "# 输出变量",
            var_decls,
            "",
            f"extern function {func_name}({params}) -> {return_type} {{",
            f'    rust_module: "{rust_module}"',
            f"    pure: {pure}",
            f"    time_bound_us: {time_bound_us}",
            "}",
            "",
            "[tasks]",
            "",
            "task main:",
            "    step invoke:",
            f"        action: call {func_name}({args_str}) {binding}",
            '        action: log "调用完成"',
            "    on_complete: goto done",
            "",
            "task done:",
            "    step hold:",
            "        allow_indefinite_wait: true",
            "```",
            "",
            "注意事项：",
            f"- `pure: {pure}` — {pure_note}",
            f"- `time_bound_us: {time_bound_us}` — 必须是实测最坏情况加余量，编译器会做 tick 预算检查",
            "- 调用只能在 `action:` 中，不能在表达式上下文中使用",
            "- 如需错误处理，在 topology 中添加 `variable last_error: int = 0`",
        ]

        return "\n".join(lines)

    @mcp.prompt()
    def pid_control_template(
        process_var: str = "temperature",
        setpoint: str = "25.0",
        kp: str = "2.0",
        ki: str = "0.5",
        kd: str = "0.1",
    ) -> str:
        """
        PID 闭环控制 .plc 模板。

        Args:
            process_var: 过程变量名称，如 "temperature"、"pressure"
            setpoint: 设定值
            kp: 比例系数
            ki: 积分系数
            kd: 微分系数
        """
        lines = [
            f"以下是 PID 闭环控制的 .plc 模板（控制对象: {process_var}）。",
            "",
            "```plc",
            "[topology]",
            "",
            "device plc_main: plc {",
            '    purpose: "控制器本体",',
            "    ports: [AI0:analog:consumer, AO0:analog:producer]",
            "}",
            "",
            f"device ai_{process_var}: analog_input {{",
            f'    purpose: "{process_var} 传感器反馈",',
            "    range: 0..100,",
            '    unit: "unit"',
            "}",
            "",
            "device ao_output: analog_output {",
            '    purpose: "控制输出（执行器命令）",',
            "    range: 0..100,",
            '    unit: "%"',
            "}",
            "",
            f"device pid_{process_var}: pid {{",
            f'    purpose: "{process_var} PID 控制器",',
            f"    pv: ai_{process_var},",
            f"    sp: {setpoint},",
            f"    kp: {kp},",
            f"    ki: {ki},",
            f"    kd: {kd},",
            "    out: ao_output,",
            "    period_ms: 100,",
            "    limit: [0.0, 100.0]",
            "}",
            "",
            "[constraints]",
            "",
            "timing: task.control_loop must_complete_within 200ms",
            "",
            "[tasks]",
            "",
            "task control_loop:",
            "    step update:",
            f"        action: pid_{process_var} update 100",
            f"        wait: pid_{process_var}.output > 0.0",
            "        timeout: 200ms -> goto fault_handler",
            "    on_complete: goto control_loop",
            "",
            "task fault_handler:",
            "    step safe:",
            "        action: set ao_output off",
            '        action: log "PID 控制异常，已关闭输出"',
            "    on_complete: goto ready",
            "",
            "task ready:",
            "    step wait_start:",
            f"        wait: ai_{process_var} >= 0.0",
            "        allow_indefinite_wait: true",
            "    on_complete: goto control_loop",
            "```",
        ]

        return "\n".join(lines)
