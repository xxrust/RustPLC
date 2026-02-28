# RustPLC MCP Server

将 RustPLC 编译器封装为 MCP (Model Context Protocol) 服务器，让任何 Claude Code 用户通过简单配置即可获得 PLC DSL 生成能力。

## 功能特性

### Tools（可执行工具）

- **get_plc_generation_guide** - 获取完整的 DSL 生成指引（SKILL.md）
- **validate_plc** - 验证 .plc 文件是否通过四大验证引擎（Safety/Liveness/Timing/Causality）
- **compile_plc** - 编译 .plc 文件并返回 IR JSON 和验证报告

### Resources（数据源）

- **rustplc://examples/\*** - 访问所有示例 .plc 文件
- **rustplc://docs/\*** - 访问技术文档
- **rustplc://skill/plc-gen** - 访问完整的生成指引

### Prompts（可复用模板）

- **generate_plc_from_description** - 从自然语言生成 .plc 程序
- **two_cylinder_template** - 双气缸顺序动作模板
- **extern_function_template** - Extern 函数声明模板
- **pid_control_template** - PID 闭环控制模板

## 快速开始

### 方式一：本地开发（推荐）

1. **构建 RustPLC 编译器**

```bash
cd /path/to/rust_plc
cargo build --release
```

2. **安装 MCP 服务器依赖**

```bash
cd rustplc-mcp
pip install mcp
```

3. **配置 Claude Code**

在项目根目录已有 `.mcp.json` 配置文件，Claude Code 会自动识别：

```json
{
  "mcpServers": {
    "rustplc": {
      "type": "stdio",
      "command": "python",
      "args": ["-m", "server"],
      "cwd": "${workspaceFolder}/rustplc-mcp",
      "env": {
        "RUSTPLC_PATH": "${workspaceFolder}/target/release/rust_plc"
      }
    }
  }
}
```

4. **重启 Claude Code**

重启后 MCP 服务器会自动连接。

### 方式二：全局安装（未来支持）

```bash
# 从 PyPI 安装（待发布）
pip install rustplc-mcp

# 添加到 Claude Code
claude mcp add --transport stdio rustplc -- python -m rustplc_mcp
```

## 使用示例

### 场景 1：从零生成 PLC 程序

```
用户: "帮我生成一个双缸顺序动作的 PLC 程序"

Claude Code 会：
1. 调用 get_plc_generation_guide 获取生成指引
2. 执行四阶段流程（.system.md → 理解工艺 → 推理拓扑 → 推导约束 → 生成 DSL）
3. 每个阶段都会等待你确认
4. 最终生成 .plc 文件并自动调用 validate_plc 验证
5. 返回验证通过的完整文件
```

### 场景 2：使用模板快速生成

```
用户: "/mcp__rustplc__two_cylinder_template button single"

Claude Code 会：
1. 返回预填充的双缸模板
2. 询问是否需要调整
3. 保存文件
```

### 场景 3：参考示例学习

```
用户: "我想看看 PID 控制怎么写"

Claude Code 会：
1. 读取 @rustplc://examples/pid_loop.plc
2. 读取 @rustplc://docs/extern_function_mvp_spec.md
3. 结合两者给你讲解
```

### 场景 4：验证现有代码

```
用户: "帮我验证这个 .plc 文件"
[粘贴代码]

Claude Code 会：
1. 调用 validate_plc 工具
2. 返回详细的验证报告
3. 如果失败，给出修复建议
```

## 可用资源速查

### 示例文件

```
@rustplc://examples/two_cylinder.plc              # 双气缸顺序动作（基础）
@rustplc://examples/assembly_station.plc          # 装配站（多设备协同）
@rustplc://examples/pid_loop.plc                  # PID 闭环控制
@rustplc://examples/nuclear_coolant_isolation.plc # 核电站隔离阀（SIL3）
@rustplc://examples/quadratic_fit.plc             # 二次函数拟合（复杂计算）
```

### 技术文档

```
@rustplc://docs/extern_function_mvp_spec.md           # Extern 函数语法规范
@rustplc://docs/extern_function_development_guide.md  # Extern 函数开发指南
@rustplc://docs/dsl_verification_boundary.md          # DSL 形式化验证边界
@rustplc://docs/device-library-design.md              # 设备库设计
@rustplc://docs/scenario_playbook.md                  # 场景系统 playbook
```

### 生成指引

```
@rustplc://skill/plc-gen         # 完整的生成指引（SKILL.md）
@rustplc://skill/plc-gen/summary # 简要摘要
```

## 可用 Prompts

### generate_plc_from_description

```
/mcp__rustplc__generate_plc_from_description "推料缸把工件推到位，传感器检测到后压紧缸下压"
```

### two_cylinder_template

```
/mcp__rustplc__two_cylinder_template button single
/mcp__rustplc__two_cylinder_template signal auto
```

参数：
- `start_mode`: "button"（按钮启动）或 "signal"（信号启动）
- `cycle_mode`: "single"（单次循环）或 "auto"（自动循环）

### extern_function_template

```
/mcp__rustplc__extern_function_template quadratic_fit "x: float, y: float" "(float, float, float)" "math::fit" true 80
```

参数：
- `func_name`: 函数名
- `params`: 参数列表
- `return_type`: 返回类型
- `rust_module`: Rust 模块路径
- `pure`: 是否纯函数（true/false）
- `time_bound_us`: 时间上界（微秒）

### pid_control_template

```
/mcp__rustplc__pid_control_template temperature 25.0 2.0 0.5 0.1
```

参数：
- `process_var`: 过程变量名称
- `setpoint`: 设定值
- `kp`: 比例系数
- `ki`: 积分系数
- `kd`: 微分系数

## 架构说明

```
rustplc-mcp/
├── server.py              # MCP 服务器入口
├── rust_bridge.py         # Rust 编译器桥接层
├── tools/
│   ├── generate.py        # 生成和验证工具
│   └── validate.py        # 验证工具（预留）
├── resources/
│   ├── examples.py        # 示例文件资源
│   ├── docs.py            # 文档资源
│   └── skill.py           # SKILL.md 资源
├── prompts/
│   └── templates.py       # 场景模板
├── pyproject.toml         # Python 项目配置
└── README.md              # 本文件
```

## 环境变量

- **RUSTPLC_PATH** - RustPLC 编译器二进制路径（可选，默认自动查找）

## 故障排查

### 问题：MCP 服务器无法启动

**解决方案：**
1. 确认已安装 `mcp` 包：`pip install mcp`
2. 确认 Python 版本 >= 3.10
3. 检查 `.mcp.json` 中的路径是否正确

### 问题：validate_plc 报错 "rustplc binary not found"

**解决方案：**
1. 确认已构建编译器：`cargo build --release`
2. 设置 `RUSTPLC_PATH` 环境变量指向编译器二进制
3. 或在 `.mcp.json` 中配置正确的路径

### 问题：无法访问示例文件

**解决方案：**
1. 确认 `examples/` 目录存在于项目根目录
2. 确认 `.mcp.json` 中的 `cwd` 路径正确

## 开发指南

### 添加新的 Tool

在 `tools/` 目录下创建新文件，定义函数并用 `@mcp.tool()` 装饰：

```python
@mcp.tool()
def my_new_tool(param: str) -> str:
    """工具描述"""
    return f"Result: {param}"
```

### 添加新的 Resource

在 `resources/` 目录下创建新文件，定义函数并用 `@mcp.resource()` 装饰：

```python
@mcp.resource("rustplc://my_resource/{id}")
def get_my_resource(id: str) -> str:
    """资源描述"""
    return f"Resource content for {id}"
```

### 添加新的 Prompt

在 `prompts/templates.py` 中添加新函数，用 `@mcp.prompt()` 装饰：

```python
@mcp.prompt()
def my_template(param: str) -> str:
    """模板描述"""
    return f"Template with {param}"
```

## 路线图

### Phase 1: MVP（已完成）
- [x] Python FastMCP 服务器框架
- [x] Tool: get_plc_generation_guide, validate_plc, compile_plc
- [x] Resource: examples/\*, docs/\*, skill/plc-gen
- [x] Prompt: 4 个常见场景模板
- [x] Stdio 传输支持
- [x] 基础文档

### Phase 2: 增强（计划中）
- [ ] Tool: simulate_plc（SIL 仿真）
- [ ] Resource: device_library（设备库查询）
- [ ] HTTP 传输支持
- [ ] Docker 镜像
- [ ] PyPI 发布

### Phase 3: 生产化（未来）
- [ ] 认证和权限控制
- [ ] 使用统计和监控
- [ ] 错误处理和重试
- [ ] 缓存和性能优化
- [ ] CI/CD 自动发布

## 贡献指南

欢迎贡献！请遵循以下步骤：

1. Fork 本仓库
2. 创建特性分支：`git checkout -b feature/my-feature`
3. 提交更改：`git commit -am 'Add my feature'`
4. 推送分支：`git push origin feature/my-feature`
5. 提交 Pull Request

## 许可证

与 RustPLC 主项目保持一致。

## 相关链接

- [RustPLC 主仓库](https://github.com/yourusername/rust_plc)
- [MCP 官方文档](https://modelcontextprotocol.io/)
- [Claude Code 文档](https://code.claude.com/)
