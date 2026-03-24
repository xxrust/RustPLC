# RustPLC MCP Server 使用演示

本文档展示如何在 Claude Code 中使用 RustPLC MCP 服务器生成和验证 PLC 程序。

## 前置准备

1. **确认 MCP 服务器已配置**
   - 项目根目录已有 `.mcp.json` 配置文件
   - 重启 Claude Code 后会自动连接

2. **确认编译器已构建**
   ```bash
   cargo build --release
   ```

3. **运行测试确认一切正常**
   ```bash
   cd rustplc-mcp
   python test_server.py
   ```

## 使用场景演示

### 场景 1：从零生成 PLC 程序（完整流程）

**用户输入：**
```
帮我生成一个双缸顺序动作的 PLC 程序。

工艺描述：
- 有两个气缸 A 和 B
- 按下启动按钮后，先伸出 A 缸，等待到位
- A 缸到位后，伸出 B 缸，等待到位
- B 缸到位后，先缩回 B 缸，等待到位
- B 缸缩回后，缩回 A 缸，等待到位
- 完成后回到等待启动状态
- 如果任何动作超时，进入故障处理
```

**Claude Code 执行流程：**

1. **调用 `get_rustplc_skill_guide` 工具**
   - 获取完整的 SKILL.md 内容
   - 了解四阶段生成流程

2. **阶段零：生成 .system.md**
   ```markdown
   ## 项目身份
   - 项目名称：双缸顺序动作演示系统
   - 所属行业：工业自动化教学
   - 部署场所：实验室教学台
   - 最终用户：自动化专业学生
   - 监管要求：无

   ## 系统使命
   演示气缸顺序控制的基本原理，用于教学和实验。
   失效后果：演示失败，无人身伤害或财产损失风险。

   ## 安全与可靠性定位
   - 安全等级：常规工业防护
   - 故障后果：演示失败
   - 容错策略：单一超时保护

   ## 运行环境
   - 介质：压缩空气 0.5MPa
   - 气源：实验室气源
   - 电源：24V DC
   - 控制器：通用 PLC
   - 通信：无
   - 环境条件：室内常温

   ## 核心工艺意图
   按钮启动后，A 缸伸出 → B 缸伸出 → B 缸缩回 → A 缸缩回，
   单次循环，完成后等待再次启动。

   ## 关键约束
   - A 缸和 B 缸不能同时伸出（如果有物理干涉）
   - 每个动作必须有超时保护
   - 超时后进入故障处理，缩回所有气缸

   ## 设计偏好
   - 命名语言：英文
   - 代码风格：清晰易读
   - 时序参数：使用默认值（气缸 300ms，阀 20ms）
   - 扩展预期：无

   ## 对 AI 的指引
   这是教学演示系统，安全等级低，使用简单的超时保护即可。
   不需要冗余传感器或复杂的容错逻辑。
   ```

   **等待用户确认：** "这个系统描述准确吗？"

3. **阶段一：理解工艺**
   ```
   动作序列：
   1. [启动按钮] 按下 → 触发循环
   2. [A 缸] 伸出 → 等待 [A 缸伸出传感器]
   3. [B 缸] 伸出 → 等待 [B 缸伸出传感器]
   4. [B 缸] 缩回 → 等待 [B 缸缩回传感器]
   5. [A 缸] 缩回 → 等待 [A 缸缩回传感器]
   6. 回到等待启动状态

   触发方式：按钮启动
   循环模式：单次循环，完成后等待再次启动
   ```

   **确认问题：**
   - "我理解的动作顺序对吗？"
   - "A 缸和 B 缸可以同时动作吗？"
   - "有没有我遗漏的动作或状态？"

   **等待用户确认**

4. **阶段二：推理设备拓扑**
   ```
   推理出的设备拓扑：

   PLC 控制器：
     - plc_main: plc { ports: [Y0, Y1, X0..X4] }

   执行机构：
     - A 缸 (cyl_A)    ← 电磁阀 (valve_A)    ← plc_main.Y0
     - B 缸 (cyl_B)    ← 电磁阀 (valve_B)    ← plc_main.Y1

   传感器：
     - A 缸伸出 (sensor_A_ext)   → plc_main.X0，检测 cyl_A.extended
     - A 缸缩回 (sensor_A_ret)   → plc_main.X1，检测 cyl_A.retracted
     - B 缸伸出 (sensor_B_ext)   → plc_main.X2，检测 cyl_B.extended
     - B 缸缩回 (sensor_B_ret)   → plc_main.X3，检测 cyl_B.retracted

   人机交互：
     - 启动按钮 (start_button)      → plc_main.X4

   默认时序参数：
     - 电磁阀响应: 20ms
     - 气缸行程: 300ms（伸出/缩回）
   ```

   **确认问题：**
   - "设备清单完整吗？"
   - "命名可以吗？"
   - "时序参数需要调整吗？"

   **等待用户确认**

5. **阶段三：推导约束**
   ```
   推导出的约束：

   安全约束（物理干涉）：
     - 需要确认：A 缸和 B 缸在物理上会干涉吗？
       如果会：safety: cyl_A.extended conflicts_with cyl_B.extended

   因果链（自动推断）：
     - Y0 -> valve_A -> cyl_A -> sensor_A_ext
     - Y1 -> valve_B -> cyl_B -> sensor_B_ext

   时序约束：
     - 整个循环应在 3 秒内完成（4 个动作 × 500ms 超时）
     - timing: task.cycle must_complete_within 3000ms
   ```

   **确认问题：**
   - "A 缸和 B 缸在物理上会干涉吗？"
   - "还有其他不能同时动作的组合吗？"
   - "整个循环有周期时间要求吗？"

   **等待用户确认**

6. **阶段四：生成 DSL 并验证**

   生成 `two_cylinder.plc` 文件，然后调用 `validate_plc` 工具验证。

   **如果验证通过：**
   ```
   ✅ 验证通过

   验证报告：
   - Safety:    通过 — conflicts_with 全部满足
   - Liveness:  通过 — 无死锁风险
   - Timing:    通过 — 关键路径 2640ms < 3000ms
   - Causality: 通过 — 所有信号链路连通
   ```

   **如果验证失败：**
   - 阅读错误信息
   - 修复问题
   - 重新验证
   - 直到全部通过

7. **最终输出**

   返回验证通过的完整 `.plc` 文件和 `.system.md` 文件。

---

### 场景 2：使用模板快速生成

**用户输入：**
```
/mcp__rustplc__two_cylinder_template button single
```

**Claude Code 执行：**
- 调用 `two_cylinder_template` prompt
- 返回预填充的双缸模板
- 询问是否需要调整
- 调用 `validate_plc` 验证

**输出：**
完整的 `.plc` 模板代码，可直接使用或根据需要调整。

---

### 场景 3：参考示例学习

**用户输入：**
```
我想学习如何使用 extern function 做复杂计算，有示例吗？
```

**Claude Code 执行：**
1. 读取 `@rustplc://examples/quadratic_fit.plc`
2. 读取 `@rustplc://docs/extern_function_mvp_spec.md`
3. 读取 `@rustplc://docs/extern_function_development_guide.md`
4. 结合三者给出详细讲解

**输出：**
- 完整的示例代码
- 语法规范说明
- 开发指南
- 使用建议

---

### 场景 4：验证现有代码

**用户输入：**
```
帮我验证这个 .plc 文件：

[topology]
device plc_main: plc {
    purpose: "test",
    ports: [Y0:digital:producer, X0:digital:consumer]
}

[constraints]

[tasks]
task main:
    step wait:
        allow_indefinite_wait: true
```

**Claude Code 执行：**
- 调用 `validate_plc` 工具
- 传入代码内容

**输出：**
```
✅ 验证通过

验证报告：
- Safety:    通过 — 无安全约束
- Liveness:  通过 — allow_indefinite_wait 已标记
- Timing:    通过 — 无时序约束
- Causality: 通过 — 无因果链
```

---

### 场景 5：生成 Extern Function 模板

**用户输入：**
```
/mcp__rustplc__extern_function_template quadratic_fit "x: float, y: float" "(float, float, float)" "math::fit" true 80
```

**Claude Code 执行：**
- 调用 `extern_function_template` prompt
- 生成完整的声明和调用示例

**输出：**
```plc
[topology]

# 输入变量
variable x: float = 0.0
variable y: float = 0.0

# 输出变量
variable out_0: float = 0.0
variable out_1: float = 0.0
variable out_2: float = 0.0

extern function quadratic_fit(x: float, y: float) -> (float, float, float) {
    rust_module: "math::fit"
    pure: true
    time_bound_us: 80
}

[tasks]

task main:
    step invoke:
        action: call quadratic_fit(x, y) -> (out_0, out_1, out_2)
        action: log "调用完成"
    on_complete: goto done

task done:
    step hold:
        allow_indefinite_wait: true
```

---

## 可用资源速查

### 访问示例文件
```
@rustplc://examples/two_cylinder.plc
@rustplc://examples/assembly_station.plc
@rustplc://examples/pid_loop.plc
@rustplc://examples/nuclear_coolant_isolation.plc
@rustplc://examples/quadratic_fit.plc
```

### 访问技术文档
```
@rustplc://docs/extern_function_mvp_spec.md
@rustplc://docs/extern_function_development_guide.md
@rustplc://docs/dsl_verification_boundary.md
@rustplc://docs/device-library-design.md
@rustplc://docs/scenario_playbook.md
```

### 访问生成指引
```
@rustplc://skill/rustplc              # 统一 skill 指引
@rustplc://skill/rustplc/summary      # 简要摘要
```

## 调试技巧

### 1. 查看 MCP 服务器日志

如果遇到问题，可以查看 MCP 服务器的日志输出（在 Claude Code 的终端中）。

### 2. 手动测试工具

可以在 Python 中手动测试工具：

```python
cd rustplc-mcp
python

from tools.generate import *
from rust_bridge import validate_plc_content

# 测试验证
plc_code = """
[topology]
device plc_main: plc {
    purpose: "test",
    ports: [X0:digital:consumer]
}

[constraints]

[tasks]
task main:
    step wait:
        allow_indefinite_wait: true
"""

result = validate_plc_content(plc_code)
print(result)
```

### 3. 检查编译器路径

```python
from rust_bridge import RUSTPLC_BIN
print(f"RustPLC binary: {RUSTPLC_BIN}")
```

## 常见问题

### Q: MCP 服务器无法启动

**A:** 检查以下几点：
1. 确认已安装 `mcp` 包：`pip install mcp`
2. 确认 Python 版本 >= 3.10
3. 检查 `.mcp.json` 中的路径是否正确
4. 重启 Claude Code

### Q: validate_plc 报错 "rustplc binary not found"

**A:** 检查以下几点：
1. 确认已构建编译器：`cargo build --release`
2. 检查 `.mcp.json` 中的 `RUSTPLC_PATH` 是否正确
3. 手动设置环境变量：`export RUSTPLC_PATH=/path/to/rust_plc.exe`

### Q: 无法访问示例文件

**A:** 检查以下几点：
1. 确认 `examples/` 目录存在于项目根目录
2. 确认 `.mcp.json` 中的 `cwd` 路径正确
3. 运行 `python test_server.py` 检查资源是否可访问

### Q: 生成的代码验证失败

**A:** 这是正常的！MCP 服务器会：
1. 阅读错误信息
2. 修复问题
3. 重新验证
4. 直到全部通过

如果多次失败，可能是：
- 工艺描述不清晰（需要更多确认）
- 约束冲突（需要调整安全约束）
- 时序不合理（需要调整超时值）

## 总结

RustPLC MCP 服务器提供了一个强大而易用的接口，让你可以：

1. ✅ 通过自然语言对话生成 PLC 程序
2. ✅ 自动验证生成的代码
3. ✅ 访问所有示例和文档
4. ✅ 使用预定义的模板快速生成
5. ✅ 学习和参考最佳实践

**开始使用：** 在 Claude Code 中直接对话，说出你的需求，MCP 服务器会引导你完成整个流程！
