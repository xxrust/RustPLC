# RustPLC MCP Server - 快速开始指南

## 前置条件

1. **Python 3.10+**
2. **RustPLC 编译器已构建**
   ```bash
   cd /path/to/rust_plc
   cargo build --release
   ```
3. **安装 MCP 包**
   ```bash
   pip install mcp
   ```

## 安装步骤

### 1. 验证安装

```bash
cd rustplc-mcp
python test_server.py
```

应该看到：
```
All tests passed! MCP server is ready.
```

### 2. 配置 Claude Code

项目根目录已有 `.mcp.json` 配置文件：

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

### 3. 重启 Claude Code

重启后 MCP 服务器会自动连接。

## 使用示例

### 示例 1：从零生成 PLC 程序

```
你: "帮我生成一个双缸顺序动作的 PLC 程序"

Claude Code 会：
1. 调用 get_plc_generation_guide 获取生成指引
2. 执行四阶段流程（.system.md → 理解工艺 → 推理拓扑 → 推导约束 → 生成 DSL）
3. 每个阶段都会等待你确认
4. 最终生成 .plc 文件并自动调用 validate_plc 验证
5. 返回验证通过的完整文件
```

### 示例 2：使用模板快速生成

```
你: "/mcp__rustplc__two_cylinder_template button single"

Claude Code 会返回预填充的双缸模板
```

### 示例 3：参考示例学习

```
你: "我想看看 PID 控制怎么写"

Claude Code 会：
1. 读取 @rustplc://examples/pid_loop.plc
2. 读取 @rustplc://docs/extern_function_mvp_spec.md
3. 结合两者给你讲解
```

### 示例 4：验证现有代码

```
你: "帮我验证这个 .plc 文件"
[粘贴代码]

Claude Code 会：
1. 调用 validate_plc 工具
2. 返回详细的验证报告
3. 如果失败，给出修复建议
```

## 可用工具

### Tools
- `get_plc_generation_guide` - 获取完整的 DSL 生成指引
- `validate_plc` - 验证 .plc 文件
- `compile_plc` - 编译并返回 IR JSON

### Resources
- `@rustplc://examples/<filename>` - 访问示例文件
- `@rustplc://docs/<filename>` - 访问技术文档
- `@rustplc://skill/plc-gen` - 访问生成指引

### Prompts
- `/mcp__rustplc__generate_plc_from_description <描述>`
- `/mcp__rustplc__two_cylinder_template <start_mode> <cycle_mode>`
- `/mcp__rustplc__extern_function_template <参数>`
- `/mcp__rustplc__pid_control_template <参数>`

## 故障排查

### 问题：MCP 服务器无法启动

**解决方案：**
1. 确认已安装 `mcp` 包：`pip install mcp`
2. 确认 Python 版本 >= 3.10
3. 检查 `.mcp.json` 中的路径是否正确

### 问题：validate_plc 报错 "rustplc binary not found"

**解决方案：**
1. 确认已构建编译器：`cargo build --release`
2. 检查 `.mcp.json` 中的 `RUSTPLC_PATH` 是否正确
3. 或手动设置环境变量：`export RUSTPLC_PATH=/path/to/rust_plc.exe`

### 问题：无法访问示例文件

**解决方案：**
1. 确认 `examples/` 目录存在于项目根目录
2. 确认 `.mcp.json` 中的 `cwd` 路径正确

## 下一步

- 查看 [README.md](README.md) 了解完整功能
- 查看 [docs/mcp_server_design.md](../docs/mcp_server_design.md) 了解架构设计
- 尝试生成你的第一个 PLC 程序！

## 反馈与贡献

遇到问题或有建议？欢迎在 GitHub 提 Issue 或 PR。
